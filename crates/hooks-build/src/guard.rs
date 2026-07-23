//! The guard pass: `docs/DESIGN.md` §6.3.
//!
//! Scans every function body for `loop` opcodes and checks that each one
//! begins with the guard prologue `i32.const a; i32.const b; call $_g`
//! (optionally followed by `drop`). This module provides both the pure
//! scanner (used by the validator's hard-error check, and by `check`) and
//! the opt-in auto-insertion transform (used by `build --auto-guard` /
//! `clean --auto-guard`).

use anyhow::{Context, Result};

use crate::Options;
use crate::ir::{self, IndexRemapper};

/// One `loop` site found while scanning a function body.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopSite {
    /// Byte offset of the `loop` opcode within the function body (from the
    /// start of the body, locals included — i.e. as returned by
    /// `OperatorsReader::read_with_offset`).
    pub offset: usize,
    /// Whether this loop already begins with a valid guard prologue.
    pub guarded: bool,
}

/// Scans a function body for `loop` sites, classifying each as guarded or
/// not. `g_index` is the current function index of the `_g` import in this
/// module, if one exists (a guard can only be recognized if its `call`
/// target actually resolves to `_g`).
pub(crate) fn scan_function_loops(
    body: &wasmparser::FunctionBody,
    g_index: Option<u32>,
) -> Result<Vec<LoopSite>> {
    let mut sites = Vec::new();
    let mut reader = body.get_operators_reader().context("function body")?;
    while !reader.eof() {
        let (op, offset) = reader
            .read_with_offset()
            .context("function body operator")?;
        if let wasmparser::Operator::Loop { .. } = op {
            let guarded = is_guard_prologue(&reader, g_index)?;
            sites.push(LoopSite { offset, guarded });
        }
    }
    Ok(sites)
}

/// Peeks (without consuming `reader`) at the next few operators to check for
/// the guard prologue `i32.const; i32.const; call $_g`.
fn is_guard_prologue(reader: &wasmparser::OperatorsReader, g_index: Option<u32>) -> Result<bool> {
    let Some(g_index) = g_index else {
        return Ok(false);
    };
    let mut r = reader.clone();
    if r.eof() {
        return Ok(false);
    }
    if !matches!(r.read()?, wasmparser::Operator::I32Const { .. }) {
        return Ok(false);
    }
    if r.eof() {
        return Ok(false);
    }
    if !matches!(r.read()?, wasmparser::Operator::I32Const { .. }) {
        return Ok(false);
    }
    if r.eof() {
        return Ok(false);
    }
    match r.read()? {
        wasmparser::Operator::Call { function_index } => Ok(function_index == g_index),
        _ => Ok(false),
    }
}

/// Finds the current function index of the `env`/`_g` import, if imported.
pub(crate) fn find_g_index(m: &ir::ParsedModule) -> Option<u32> {
    m.find_func_import("env", "_g")
}

/// Runs the auto-guard transform: for every function, any `loop` missing a
/// guard prologue gets one inserted immediately after the loop's blocktype:
/// `i32.const id; i32.const maxiter; call $_g; drop`, where
/// `id = (1 << 30) + n` (`n` sequential across the whole module, disjoint
/// from the `guard!`/`guard_m!` id space which starts at `1 << 31`) and
/// `maxiter = opts.default_maxiter`.
///
/// If `_g` is not already imported, it is added (as the last import), which
/// uniformly shifts every defined function index by one; every `call` and
/// export index in the module is updated accordingly.
///
/// If every loop already carries a guard, `wasm` is returned unchanged
/// (byte-for-byte) — this keeps the pass idempotent and avoids
/// re-encoding a module that needed no changes.
pub fn auto_guard(wasm: &[u8], opts: &Options) -> Result<Vec<u8>> {
    let m = ir::parse(wasm)?;
    let existing_g_index = find_g_index(&m);

    // First pass: does anything actually need inserting? If not, return the
    // input unchanged.
    let mut any_unguarded = false;
    for body in &m.code {
        let sites = scan_function_loops(body, existing_g_index)?;
        if sites.iter().any(|s| !s.guarded) {
            any_unguarded = true;
            break;
        }
    }
    if !any_unguarded {
        return Ok(wasm.to_vec());
    }

    /// Whether `_g` is already imported, or needs to be added (and if so,
    /// at which new function index and with which type index).
    #[derive(Clone, Copy)]
    enum GImport {
        Existing(u32),
        New { g_index: u32, type_idx: u32 },
    }

    let n_imp_funcs = m.num_imported_funcs();
    let mut types = m.types.clone();
    let g_import = match existing_g_index {
        Some(idx) => GImport::Existing(idx),
        None => {
            let type_idx = ir::find_or_insert_type(
                &mut types,
                &[wasmparser::ValType::I32, wasmparser::ValType::I32],
                &[wasmparser::ValType::I32],
            );
            GImport::New {
                g_index: n_imp_funcs,
                type_idx,
            }
        }
    };
    let (g_index, need_new_import, shift_at) = match g_import {
        GImport::Existing(idx) => (idx, false, None),
        GImport::New { g_index, .. } => (g_index, true, Some(g_index)),
    };
    let func_map = move |old: u32| -> u32 {
        match shift_at {
            Some(threshold) if old >= threshold => old + 1,
            _ => old,
        }
    };
    let global_map = |old: u32| old;

    let mut counter: u32 = 0;

    let mut module = wasm_encoder::Module::new();

    let mut types_sec = wasm_encoder::TypeSection::new();
    for ty in &types {
        let (params, results) = ir::conv_functype(ty)?;
        types_sec.ty().function(params, results);
    }
    module.section(&types_sec);

    let mut imports_sec = wasm_encoder::ImportSection::new();
    for imp in &m.imports {
        match imp.ty {
            wasmparser::TypeRef::Func(type_idx) => {
                imports_sec.import(
                    imp.module,
                    imp.name,
                    wasm_encoder::EntityType::Function(type_idx),
                );
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
                anyhow::bail!("unsupported import: tag imports are not supported");
            }
            wasmparser::TypeRef::FuncExact(_) => {
                anyhow::bail!(
                    "unsupported import: exact function-reference imports are not supported"
                );
            }
        }
    }
    if let GImport::New { type_idx, .. } = g_import {
        imports_sec.import("env", "_g", wasm_encoder::EntityType::Function(type_idx));
    }
    module.section(&imports_sec);

    let mut funcs_sec = wasm_encoder::FunctionSection::new();
    for &type_idx in &m.defined_func_types {
        funcs_sec.function(type_idx);
    }
    module.section(&funcs_sec);

    let mut mem_sec = wasm_encoder::MemorySection::new();
    for &mem in &m.memories {
        mem_sec.memory(ir::conv_memtype(mem));
    }
    module.section(&mem_sec);

    let mut globals_sec = wasm_encoder::GlobalSection::new();
    {
        let mut remapper = IndexRemapper::new(func_map, global_map);
        for g in &m.globals {
            let ty = ir::conv_globaltype(g.ty)?;
            let expr = ir::remap_const_expr(&g.init_expr, &mut remapper)?;
            globals_sec.global(ty, &expr);
        }
    }
    module.section(&globals_sec);

    let mut exports_sec = wasm_encoder::ExportSection::new();
    for e in &m.exports {
        let kind = match e.kind {
            wasmparser::ExternalKind::Func => wasm_encoder::ExportKind::Func,
            wasmparser::ExternalKind::Table => wasm_encoder::ExportKind::Table,
            wasmparser::ExternalKind::Memory => wasm_encoder::ExportKind::Memory,
            wasmparser::ExternalKind::Global => wasm_encoder::ExportKind::Global,
            wasmparser::ExternalKind::Tag => wasm_encoder::ExportKind::Tag,
            wasmparser::ExternalKind::FuncExact => anyhow::bail!(
                "unsupported export: exact function-reference exports are not supported"
            ),
        };
        let index = if matches!(e.kind, wasmparser::ExternalKind::Func) {
            func_map(e.index)
        } else {
            e.index
        };
        exports_sec.export(e.name, kind, index);
    }
    module.section(&exports_sec);

    // Section order is fixed by the wasm spec: code precedes data.
    let mut code_sec = wasm_encoder::CodeSection::new();
    {
        let mut remapper = IndexRemapper::new(func_map, global_map);
        for body in &m.code {
            let sites = scan_function_loops(body, existing_g_index)?;
            let unguarded_offsets: std::collections::BTreeSet<usize> = sites
                .iter()
                .filter(|s| !s.guarded)
                .map(|s| s.offset)
                .collect();

            if unguarded_offsets.is_empty() && !need_new_import {
                // Nothing in this body changes at all.
                if !ir::body_needs_remap(body, &func_map, &global_map)? {
                    code_sec.raw(body.as_bytes());
                    continue;
                }
            }

            let built = ir::rebuild_function_body(body, &mut remapper, |op, offset, func| {
                if matches!(op, wasmparser::Operator::Loop { .. })
                    && unguarded_offsets.contains(&offset)
                {
                    let id = (1u32 << 30) + counter;
                    counter += 1;
                    func.instruction(&wasm_encoder::Instruction::I32Const(id as i32));
                    func.instruction(&wasm_encoder::Instruction::I32Const(
                        opts.default_maxiter as i32,
                    ));
                    func.instruction(&wasm_encoder::Instruction::Call(g_index));
                    func.instruction(&wasm_encoder::Instruction::Drop);
                }
                Ok(())
            })?;
            code_sec.function(&built);
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
