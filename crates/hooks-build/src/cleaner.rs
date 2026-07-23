//! The hook-cleaner pass: `docs/DESIGN.md` §6.2.
//!
//! Turns cargo's raw wasm output into a SetHook-shaped module: drops custom
//! sections, restricts exports to `hook`/`cbak`, garbage-collects
//! unreachable functions and globals, and re-encodes the module with a
//! whole-module index remap (one table per index space, applied everywhere
//! that space is referenced).

use std::collections::{BTreeSet, HashMap};

use anyhow::{Context, Result, bail};

use crate::Options;
use crate::ir::{self, IndexRemapper};

/// Cleans `wasm`, producing a module whose only exports are `hook` (and
/// `cbak`, if present in the input), with all reachable-from-those-exports
/// functions and globals kept (and everything else — custom sections, the
/// table, element segments, unreachable code — dropped).
///
/// Errors if the `hook` export is missing, or if `hook`/`cbak` do not have
/// the required `(i32) -> i64` signature.
pub fn clean(wasm: &[u8], _opts: &Options) -> Result<Vec<u8>> {
    let m = ir::parse(wasm)?;

    // --- Locate and validate the retained exports (GC roots). ---
    let hook_old_idx = m
        .exports
        .iter()
        .find(|e| e.name == "hook" && matches!(e.kind, wasmparser::ExternalKind::Func))
        .map(|e| e.index)
        .context("module is missing the required `hook` export")?;
    check_entry_signature(&m, hook_old_idx, "hook")?;

    let cbak_old_idx = m
        .exports
        .iter()
        .find(|e| e.name == "cbak" && matches!(e.kind, wasmparser::ExternalKind::Func))
        .map(|e| e.index);
    if let Some(idx) = cbak_old_idx {
        check_entry_signature(&m, idx, "cbak")?;
    }

    let mut roots = vec![hook_old_idx];
    if let Some(idx) = cbak_old_idx {
        roots.push(idx);
    }

    // --- Reachability: functions reachable via direct `call`, globals
    // touched by any reachable function. ---
    let mut reachable_funcs: BTreeSet<u32> = BTreeSet::new();
    let mut used_globals: BTreeSet<u32> = BTreeSet::new();
    let mut stack = roots.clone();
    while let Some(f) = stack.pop() {
        if !reachable_funcs.insert(f) {
            continue;
        }
        if let Some(body) = m.defined_body(f) {
            let refs = ir::scan_refs(body)?;
            for c in refs.calls {
                stack.push(c);
            }
            used_globals.extend(refs.globals);
        }
    }

    // Active data segments are always retained, so any global their offset
    // expression references is reachable too.
    for d in &m.datas {
        if let wasmparser::DataKind::Active { offset_expr, .. } = &d.kind
            && let Some(g) = ir::const_expr_global_ref(offset_expr)?
        {
            used_globals.insert(g);
        }
    }

    // Fixed point: a kept global's own initializer may reference another
    // (earlier) global.
    loop {
        let mut added = false;
        for (i, g) in m.globals.iter().enumerate() {
            let old_idx = m.num_imported_globals() + i as u32;
            if used_globals.contains(&old_idx)
                && let Some(r) = ir::const_expr_global_ref(&g.init_expr)?
                && used_globals.insert(r)
            {
                added = true;
            }
        }
        if !added {
            break;
        }
    }

    // --- Build whole-module index remaps. Imported functions are eligible
    // for GC exactly like defined ones (their relative order is preserved);
    // imported tables/memories/globals are always kept as-is (they are
    // hard errors downstream regardless, so GC'ing them is not worth the
    // complexity). ---
    let total_funcs = m.total_funcs();
    let mut func_new_index: HashMap<u32, u32> = HashMap::new();
    {
        let mut next = 0u32;
        for old in 0..total_funcs {
            if reachable_funcs.contains(&old) {
                func_new_index.insert(old, next);
                next += 1;
            }
        }
    }
    let n_imp_globals = m.num_imported_globals();
    let total_globals = m.total_globals();
    let mut global_new_index: HashMap<u32, u32> = HashMap::new();
    {
        let mut next = 0u32;
        for old in 0..total_globals {
            let keep = old < n_imp_globals || used_globals.contains(&old);
            if keep {
                global_new_index.insert(old, next);
                next += 1;
            }
        }
    }
    let func_map = |old: u32| -> u32 { *func_new_index.get(&old).unwrap_or(&old) };
    let global_map = |old: u32| -> u32 { *global_new_index.get(&old).unwrap_or(&old) };

    if !m.datas.is_empty() && m.memories.is_empty() {
        bail!("module has data segments but no memory is defined");
    }

    // --- Emit the cleaned module. ---
    let mut module = wasm_encoder::Module::new();

    let mut types_sec = wasm_encoder::TypeSection::new();
    for ty in &m.types {
        let (params, results) = ir::conv_functype(ty)?;
        types_sec.ty().function(params, results);
    }
    module.section(&types_sec);

    let mut imports_sec = wasm_encoder::ImportSection::new();
    let mut func_import_counter = 0u32;
    for imp in &m.imports {
        match imp.ty {
            wasmparser::TypeRef::Func(type_idx) => {
                let old_idx = func_import_counter;
                func_import_counter += 1;
                if reachable_funcs.contains(&old_idx) {
                    imports_sec.import(
                        imp.module,
                        imp.name,
                        wasm_encoder::EntityType::Function(type_idx),
                    );
                }
            }
            wasmparser::TypeRef::Table(t) => {
                imports_sec.import(imp.module, imp.name, ir::conv_tabletype(t)?);
            }
            wasmparser::TypeRef::Memory(mt) => {
                imports_sec.import(imp.module, imp.name, ir::conv_memtype(mt));
            }
            wasmparser::TypeRef::Global(gt) => {
                imports_sec.import(imp.module, imp.name, ir::conv_globaltype(gt)?);
            }
            wasmparser::TypeRef::Tag(_) => {
                bail!("unsupported import: tag imports are not supported");
            }
            wasmparser::TypeRef::FuncExact(_) => {
                bail!("unsupported import: exact function-reference imports are not supported");
            }
        }
    }
    module.section(&imports_sec);

    let n_imp_funcs = m.num_imported_funcs();
    let mut funcs_sec = wasm_encoder::FunctionSection::new();
    let mut kept_defined: Vec<(u32, &wasmparser::FunctionBody)> = Vec::new();
    for (i, (&type_idx, body)) in m.defined_func_types.iter().zip(m.code.iter()).enumerate() {
        let old_idx = n_imp_funcs + i as u32;
        if reachable_funcs.contains(&old_idx) {
            funcs_sec.function(type_idx);
            kept_defined.push((old_idx, body));
        }
    }
    module.section(&funcs_sec);

    // Tables and element segments are always dropped (call_indirect is
    // banned, so a table can never be a legitimate reachability root).

    let mut mem_sec = wasm_encoder::MemorySection::new();
    for &mem in &m.memories {
        mem_sec.memory(ir::conv_memtype(mem));
    }
    module.section(&mem_sec);

    let mut globals_sec = wasm_encoder::GlobalSection::new();
    {
        let mut remapper = IndexRemapper::new(func_map, global_map);
        for (i, g) in m.globals.iter().enumerate() {
            let old_idx = n_imp_globals + i as u32;
            if global_new_index.contains_key(&old_idx) {
                let ty = ir::conv_globaltype(g.ty)?;
                let expr = ir::remap_const_expr(&g.init_expr, &mut remapper)?;
                globals_sec.global(ty, &expr);
            }
        }
    }
    module.section(&globals_sec);

    let mut exports_sec = wasm_encoder::ExportSection::new();
    exports_sec.export(
        "hook",
        wasm_encoder::ExportKind::Func,
        func_map(hook_old_idx),
    );
    if let Some(idx) = cbak_old_idx {
        exports_sec.export("cbak", wasm_encoder::ExportKind::Func, func_map(idx));
    }
    module.section(&exports_sec);

    // No start section (banned outright; validator will reject a stray one
    // in `check` input, but the cleaner never re-emits one it saw).

    // Section order is fixed by the wasm spec: code precedes data.
    let mut code_sec = wasm_encoder::CodeSection::new();
    {
        let mut remapper = IndexRemapper::new(func_map, global_map);
        for (_old_idx, body) in &kept_defined {
            if ir::body_needs_remap(body, &func_map, &global_map)? {
                let built = ir::rebuild_function_body(body, &mut remapper, |_, _, _| Ok(()))?;
                code_sec.function(&built);
            } else {
                code_sec.raw(body.as_bytes());
            }
        }
    }
    module.section(&code_sec);

    let mut data_sec = wasm_encoder::DataSection::new();
    {
        let mut remapper = IndexRemapper::new(func_map, global_map);
        for d in &m.datas {
            match &d.kind {
                wasmparser::DataKind::Active {
                    memory_index,
                    offset_expr,
                } => {
                    let expr = ir::remap_const_expr(offset_expr, &mut remapper)?;
                    data_sec.active(*memory_index, &expr, d.data.iter().copied());
                }
                wasmparser::DataKind::Passive => {
                    data_sec.passive(d.data.iter().copied());
                }
            }
        }
    }
    module.section(&data_sec);

    Ok(module.finish())
}

/// Verifies that function `idx` has the required `hook`/`cbak` signature:
/// exactly `(i32) -> i64`.
fn check_entry_signature(m: &ir::ParsedModule, idx: u32, export_name: &str) -> Result<()> {
    let type_idx = m
        .func_type_index(idx)
        .with_context(|| format!("`{export_name}` export does not refer to a function"))?;
    let ty = m
        .types
        .get(type_idx as usize)
        .with_context(|| format!("`{export_name}` export has an invalid type index"))?;
    if ty.params() != [wasmparser::ValType::I32] || ty.results() != [wasmparser::ValType::I64] {
        bail!(
            "`{export_name}` must have signature `(i32) -> i64`, found `({:?}) -> {:?}`",
            ty.params(),
            ty.results()
        );
    }
    Ok(())
}
