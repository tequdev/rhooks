//! The SetHook validator: `docs/DESIGN.md` §6.4.
//!
//! Applies the full SetHook-derived hard-error and warning rule set to a
//! module's *final* bytes. Used both as the last stage of `build`/`clean`
//! (after cleaning and the optional guard pass) and as the entirety of
//! `check`, which runs it against arbitrary wasm (including C-built hooks).

use std::collections::HashSet;

use anyhow::{Result, bail};

use crate::guard::{find_g_index, scan_function_loops};
use crate::ir;
use crate::{ApiVersion, Options};

/// The maximum size, in bytes, of a SetHook-legal wasm binary.
pub const MAX_SIZE: usize = 65_535;

/// Sizes at or above this many bytes trigger a "getting close to the limit"
/// warning.
pub const SIZE_WARNING_THRESHOLD: usize = 56 * 1024;

/// The result of a successful validation: any non-fatal warnings found.
#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    /// Human-readable warning messages.
    pub warnings: Vec<String>,
    /// True if the module exceeded [`MAX_SIZE`] but was allowed through by
    /// `opts.allow_oversize` (only ever set outside of `check`).
    pub oversize_allowed: bool,
}

/// Validates `wasm` against the full SetHook rule set. Returns `Ok` (with
/// any warnings) if it is SetHook-legal, or `Err` describing every hard
/// error found otherwise.
pub fn validate(wasm: &[u8], opts: &Options) -> Result<ValidationReport> {
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut oversize_allowed = false;

    // --- Size. ---
    if wasm.len() > MAX_SIZE {
        if opts.allow_oversize {
            oversize_allowed = true;
            warnings.push(format!(
                "INVALID: module is {} bytes, exceeding the {MAX_SIZE}-byte SetHook limit \
                 (written anyway because --allow-oversize was given)",
                wasm.len()
            ));
        } else {
            errors.push(format!(
                "module is {} bytes, exceeding the {MAX_SIZE}-byte SetHook limit",
                wasm.len()
            ));
        }
    } else if wasm.len() >= SIZE_WARNING_THRESHOLD {
        warnings.push(format!(
            "module is {} bytes, approaching the {MAX_SIZE}-byte SetHook limit",
            wasm.len()
        ));
    }

    // --- Generic WASM validity, restricted to the MVP feature set. This
    // catches float *types* (not just opcodes), any post-MVP encoding
    // (bulk-memory, sign-extension, reference-types, SIMD, multi-value,
    // multi-memory, tail-call, exceptions, GC, component-model, ...), and
    // general structural soundness (e.g. a `start` function with the wrong
    // signature). ---
    if let Err(e) = wasmparser::Validator::new_with_features(mvp_features()).validate_all(wasm) {
        errors.push(format!("wasm is not valid under the MVP feature set: {e}"));
    }

    // The rest of the rules need our own parse; if that itself fails there
    // is nothing more we can usefully check.
    let m = match ir::parse(wasm) {
        Ok(m) => m,
        Err(e) => {
            errors.push(format!("failed to parse module: {e}"));
            bail!(errors.join("\n"));
        }
    };

    // --- Export set. ---
    let mut hook_idx = None;
    let mut cbak_idx = None;
    for e in &m.exports {
        match (e.name, e.kind) {
            ("hook", wasmparser::ExternalKind::Func) => hook_idx = Some(e.index),
            ("cbak", wasmparser::ExternalKind::Func) => cbak_idx = Some(e.index),
            _ => errors.push(format!(
                "unexpected export `{}` (only `hook` and `cbak` may be exported)",
                e.name
            )),
        }
    }
    match hook_idx {
        None => errors.push("missing required `hook` export".to_string()),
        Some(idx) => check_entry_signature(&m, idx, "hook", &mut errors),
    }
    if let Some(idx) = cbak_idx {
        check_entry_signature(&m, idx, "cbak", &mut errors);
    }

    // --- Imports: module must be `env`, name must be whitelisted, and the
    // signature must match exactly. No imported memories/tables/globals. ---
    for imp in &m.imports {
        if imp.module != "env" {
            errors.push(format!(
                "import `{}::{}` is not from module `env`",
                imp.module, imp.name
            ));
            continue;
        }
        match imp.ty {
            wasmparser::TypeRef::Func(type_idx) => match crate::whitelist::lookup(imp.name) {
                None => errors.push(format!(
                    "import `{}` is not a recognized Hook API function",
                    imp.name
                )),
                Some(entry) => {
                    if let Some(ty) = m.types.get(type_idx as usize) {
                        if !signature_matches(ty, entry) {
                            errors.push(format!(
                                "import `{}` has signature `({:?}) -> {:?}`, expected `({:?}) -> {:?}`",
                                imp.name,
                                ty.params(),
                                ty.results(),
                                entry.params,
                                entry.result
                            ));
                        }
                    } else {
                        errors.push(format!("import `{}` has an invalid type index", imp.name));
                    }
                }
            },
            wasmparser::TypeRef::Table(_) => {
                errors.push(format!(
                    "import `{}` is an imported table (not allowed)",
                    imp.name
                ));
            }
            wasmparser::TypeRef::Memory(_) => {
                errors.push(format!(
                    "import `{}` is an imported memory (not allowed)",
                    imp.name
                ));
            }
            wasmparser::TypeRef::Global(_) => {
                errors.push(format!(
                    "import `{}` is an imported global (not allowed)",
                    imp.name
                ));
            }
            wasmparser::TypeRef::Tag(_) => {
                errors.push(format!(
                    "import `{}` is a tag import (not allowed)",
                    imp.name
                ));
            }
            wasmparser::TypeRef::FuncExact(_) => {
                errors.push(format!(
                    "import `{}` is an exact function-reference import (not allowed)",
                    imp.name
                ));
            }
        }
    }

    // --- No start section. ---
    if m.start.is_some() {
        errors.push("module has a `start` section (not allowed)".to_string());
    }

    // --- Element segments: only the MVP active/function-index form is
    // tolerated to exist at all (the cleaner always drops the table and
    // every element segment, so any survivor here is either external input
    // or beyond the cleaner's scope). ---
    for (i, el) in m.elements.iter().enumerate() {
        let ok = matches!(
            (&el.kind, &el.items),
            (
                wasmparser::ElementKind::Active {
                    table_index: None | Some(0),
                    ..
                },
                wasmparser::ElementItems::Functions(_)
            )
        );
        if !ok {
            errors.push(format!(
                "element segment {i} is not in the MVP active/function-index form (passive, expression, or multi-table segments are not allowed)"
            ));
        }
    }

    // --- No data-count section. ---
    if m.data_count.is_some() {
        errors.push("module has a data-count section (not allowed)".to_string());
    }

    // --- Passive data segments; memory count. ---
    let mut has_passive_data = false;
    for d in &m.datas {
        if matches!(d.kind, wasmparser::DataKind::Passive) {
            has_passive_data = true;
        }
    }
    if has_passive_data {
        errors.push("module has a passive data segment (not allowed)".to_string());
    }
    let total_memories = m.memories.len()
        + m.imports
            .iter()
            .filter(|i| matches!(i.ty, wasmparser::TypeRef::Memory(_)))
            .count();
    if total_memories > 1 {
        errors.push(format!(
            "module defines {total_memories} memories (at most one is allowed)"
        ));
    } else if total_memories == 0 && !m.datas.is_empty() {
        errors.push("module has data segments but no memory is defined".to_string());
    }

    // --- Float opcodes/types (belt-and-suspenders alongside the generic
    // MVP-feature validation above, with function-level detail). ---
    for (i, body) in m.code.iter().enumerate() {
        let func_idx = m.num_imported_funcs() + i as u32;
        if let Ok(locals) = body.get_locals_reader() {
            for l in locals {
                if let Ok((_, ty)) = l
                    && matches!(ty, wasmparser::ValType::F32 | wasmparser::ValType::F64)
                {
                    errors.push(format!(
                        "function {func_idx} declares a floating-point local"
                    ));
                }
            }
        }
        if let Ok(mut reader) = body.get_operators_reader() {
            while !reader.eof() {
                let Ok((op, offset)) = reader.read_with_offset() else {
                    break;
                };
                let dbg = format!("{op:?}");
                if dbg.contains("F32") || dbg.contains("F64") {
                    errors.push(format!(
                        "function {func_idx} uses a floating-point opcode at offset {offset}"
                    ));
                }
                if matches!(op, wasmparser::Operator::CallIndirect { .. }) {
                    errors.push(format!(
                        "function {func_idx} uses `call_indirect` at offset {offset} (not allowed)"
                    ));
                }
            }
        }
    }

    // --- Recursion: DFS over the direct-call graph must be acyclic. ---
    if let Some(cycle) = find_call_cycle(&m) {
        errors.push(format!(
            "recursive call cycle detected among functions: {cycle:?} (recursion is not allowed)"
        ));
    }

    // --- Guards (API version 0 only). ---
    if opts.api_version == ApiVersion::V0 {
        let g_index = find_g_index(&m);
        for (i, body) in m.code.iter().enumerate() {
            let func_idx = m.num_imported_funcs() + i as u32;
            match scan_function_loops(body, g_index) {
                Ok(sites) => {
                    for site in sites.iter().filter(|s| !s.guarded) {
                        errors.push(format!(
                            "function {func_idx}, offset {}: `loop` is missing a guard (`i32.const; i32.const; call $_g`)",
                            site.offset
                        ));
                    }
                }
                Err(e) => errors.push(format!(
                    "function {func_idx}: failed to scan for guards: {e}"
                )),
            }
        }
    }

    // --- Warning: more than one mutable defined global (beyond the single
    // shadow-stack-pointer pattern). ---
    let mutable_defined_globals = m.globals.iter().filter(|g| g.ty.mutable).count();
    if mutable_defined_globals > 1 {
        warnings.push(format!(
            "module has {mutable_defined_globals} mutable globals (expected at most one, the shadow stack pointer)"
        ));
    }

    if !errors.is_empty() {
        bail!(errors.join("\n"));
    }

    Ok(ValidationReport {
        warnings,
        oversize_allowed,
    })
}

/// WASM features restricted to (approximately) the 1.0 MVP, plus explicit
/// floating-point disallowance. `mutable_global` is left enabled: internally
/// mutable globals have been part of the module encoding since the MVP
/// (only cross-module global mutability was the later "mutable globals"
/// proposal's concern).
fn mvp_features() -> wasmparser::WasmFeatures {
    wasmparser::WasmFeatures::MUTABLE_GLOBAL
}

fn check_entry_signature(
    m: &ir::ParsedModule,
    idx: u32,
    export_name: &str,
    errors: &mut Vec<String>,
) {
    let Some(type_idx) = m.func_type_index(idx) else {
        errors.push(format!(
            "`{export_name}` export does not refer to a function"
        ));
        return;
    };
    let Some(ty) = m.types.get(type_idx as usize) else {
        errors.push(format!("`{export_name}` export has an invalid type index"));
        return;
    };
    if ty.params() != [wasmparser::ValType::I32] || ty.results() != [wasmparser::ValType::I64] {
        errors.push(format!(
            "`{export_name}` must have signature `(i32) -> i64`, found `({:?}) -> {:?}`",
            ty.params(),
            ty.results()
        ));
    }
}

fn signature_matches(ty: &wasmparser::FuncType, entry: &crate::whitelist::ApiFn) -> bool {
    if ty.params().len() != entry.params.len() {
        return false;
    }
    for (a, b) in ty.params().iter().zip(entry.params.iter()) {
        if !valtype_eq(*a, *b) {
            return false;
        }
    }
    match ty.results() {
        [r] => valtype_eq(*r, entry.result),
        _ => false,
    }
}

fn valtype_eq(a: wasmparser::ValType, b: wasm_encoder::ValType) -> bool {
    matches!(
        (a, b),
        (wasmparser::ValType::I32, wasm_encoder::ValType::I32)
            | (wasmparser::ValType::I64, wasm_encoder::ValType::I64)
            | (wasmparser::ValType::F32, wasm_encoder::ValType::F32)
            | (wasmparser::ValType::F64, wasm_encoder::ValType::F64)
    )
}

/// DFS-based cycle detection over the direct-call graph (imports are leaves
/// with no out-edges). Returns one cycle (as a `Vec` of function indices)
/// if any exists.
fn find_call_cycle(m: &ir::ParsedModule) -> Option<Vec<u32>> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Color {
        White,
        Gray,
        Black,
    }

    let total = m.total_funcs();
    let mut color = vec![Color::White; total as usize];
    let mut path: Vec<u32> = Vec::new();

    // Treats an out-of-range index (only possible for already-malformed
    // input the generic validator will separately reject) as "no such
    // node" rather than panicking.
    let get = |color: &[Color], idx: u32| color.get(idx as usize).copied().unwrap_or(Color::Black);
    let set = |color: &mut [Color], idx: u32, c: Color| {
        if let Some(slot) = color.get_mut(idx as usize) {
            *slot = c;
        }
    };

    // Iterative DFS to avoid unbounded recursion on adversarial input.
    for start in 0..total {
        if get(&color, start) != Color::White {
            continue;
        }
        let mut stack: Vec<(u32, Vec<u32>)> = vec![(start, out_edges(m, start))];
        set(&mut color, start, Color::Gray);
        path.push(start);
        while let Some((node, edges)) = stack.last_mut() {
            let node = *node;
            if let Some(next) = edges.pop() {
                match get(&color, next) {
                    Color::White => {
                        set(&mut color, next, Color::Gray);
                        path.push(next);
                        let next_edges = out_edges(m, next);
                        stack.push((next, next_edges));
                    }
                    Color::Gray => {
                        let cycle_start = path.iter().position(|&p| p == next).unwrap_or(0);
                        let mut cycle = path.get(cycle_start..).unwrap_or(&[]).to_vec();
                        cycle.push(next);
                        return Some(cycle);
                    }
                    Color::Black => {}
                }
            } else {
                set(&mut color, node, Color::Black);
                path.pop();
                stack.pop();
            }
        }
    }
    None
}

fn out_edges(m: &ir::ParsedModule, idx: u32) -> Vec<u32> {
    match m.defined_body(idx) {
        Some(body) => match ir::scan_refs(body) {
            Ok(refs) => refs
                .calls
                .into_iter()
                .collect::<HashSet<_>>()
                .into_iter()
                .collect(),
            Err(_) => Vec::new(),
        },
        None => Vec::new(),
    }
}
