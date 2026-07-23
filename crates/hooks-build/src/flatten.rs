//! The flatten (full-inlining) pass: `docs/DESIGN.md` §6.2b.
//!
//! Runs for api-version 0 only, after the cleaner and before the guard pass.
//! It exists to satisfy two rules of the real (vendored) checker that the
//! Rust reimplementation originally missed:
//!
//! - **R1**: every api-version-0 module must import `_g`, even if it
//!   contains no loop at all.
//! - **R2**: every entry in the type section must be the type of an import
//!   or the `(i32) -> i64` entry-point type — a defined helper function with
//!   any other signature (notably `compiler_builtins` `memset`/`memcpy`/
//!   `bcmp`, `(i32,i32,i32) -> i32`, which rustc emits under fat LTO even
//!   with `#![no_builtins]`) makes the whole module invalid.
//!
//! The fix for R2 is to inline every defined non-entry function into its
//! callers (bottom-up, in reverse topological order over the call graph —
//! which is acyclic, since recursion is a hard error checked before
//! flattening even starts) and then drop them, leaving only the entry
//! functions (`hook`, and `cbak` if present) as defined functions. The type
//! section is then rebuilt to contain exactly the types imports need plus
//! the entry-point type, which trivially satisfies R2. R1 is handled in the
//! same rebuild: `_g` is added as an import if not already present (and,
//! being an import from here on, can never be GC'd away by anything
//! downstream).
//!
//! # Precondition
//!
//! `wasm` is expected to already be cleaner-shaped (single `hook` export,
//! optional `cbak`, no tables/elements). This module does not re-run the
//! cleaner's reachability GC; it only concerns itself with the function
//! index space.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use wasm_encoder::reencode::Reencode;

use crate::ir;

/// Report from a [`flatten`] run: which callee bodies were duplicated into
/// more than one call site (a size cost that is acceptable, per
/// `docs/DESIGN.md` §6.2b, but worth surfacing for transparency).
#[derive(Debug, Clone, Default)]
pub struct FlattenReport {
    /// Human-readable notes, e.g. "function 3 was inlined into 2 call sites
    /// (body duplicated)".
    pub notes: Vec<String>,
}

/// A defined function's state during flattening: its parameter/result
/// types (fixed, from the original type section) and its *current* body
/// and extra (non-parameter) locals, which are progressively rewritten in
/// place as callees are inlined into it. Indexed throughout by the
/// function's *original* (pre-flatten) function index — flatten never
/// renumbers functions until the very end, so callee lookups by original
/// `Call` target just work.
struct FlatFunc<'a> {
    param_types: Vec<wasmparser::ValType>,
    result_type: Option<wasmparser::ValType>,
    extra_locals: Vec<wasmparser::ValType>,
    body: Vec<wasmparser::Operator<'a>>,
}

/// Runs the flatten pass on already-cleaned `wasm`. Returns the flattened
/// bytes (with `_g` guaranteed present as an import, and a type section
/// containing exactly {import types} ∪ {entry type}) plus a report of any
/// callee bodies that were duplicated into multiple call sites.
///
/// Errors if the module has no `hook` export, or if the direct-call graph
/// among defined functions contains a cycle (recursion is a hard error,
/// checked here — before flattening even starts — because the inliner's
/// termination depends on the graph being a DAG).
pub fn flatten(wasm: &[u8]) -> Result<(Vec<u8>, FlattenReport)> {
    let m = ir::parse(wasm)?;

    let hook_old = m
        .exports
        .iter()
        .find(|e| e.name == "hook" && matches!(e.kind, wasmparser::ExternalKind::Func))
        .map(|e| e.index)
        .context("module is missing the required `hook` export")?;
    let cbak_old = m
        .exports
        .iter()
        .find(|e| e.name == "cbak" && matches!(e.kind, wasmparser::ExternalKind::Func))
        .map(|e| e.index);

    // Recursion is banned module-wide; the inliner's termination relies on
    // the call graph being a DAG, so this is checked *before* flattening
    // (docs/DESIGN.md §6.2b), not just left to the post-transform validator.
    if let Some(cycle) = ir::find_call_cycle(&m) {
        bail!(
            "recursive call cycle detected among functions: {cycle:?} (recursion is not \
             allowed; the flatten pass cannot terminate on a cyclic call graph)"
        );
    }

    let n_imp_funcs = m.num_imported_funcs();
    let total_funcs = m.total_funcs();

    // --- Build the initial per-function state, keyed by original index. ---
    let mut funcs: HashMap<u32, FlatFunc> = HashMap::new();
    for (i, (&type_idx, body)) in m.defined_func_types.iter().zip(m.code.iter()).enumerate() {
        let old_idx = n_imp_funcs + i as u32;
        let ty = m
            .types
            .get(type_idx as usize)
            .with_context(|| format!("function {old_idx} has an invalid type index"))?;
        let mut extra_locals = Vec::new();
        for l in body.get_locals_reader().context("function locals")? {
            let (count, ty) = l.context("function locals")?;
            for _ in 0..count {
                extra_locals.push(ty);
            }
        }
        let mut ops = Vec::new();
        let mut reader = body.get_operators_reader().context("function body")?;
        while !reader.eof() {
            ops.push(reader.read().context("function body operator")?);
        }
        funcs.insert(
            old_idx,
            FlatFunc {
                param_types: ty.params().to_vec(),
                result_type: ty.results().first().copied(),
                extra_locals,
                body: ops,
            },
        );
    }

    // --- Reverse topological order: callees before their callers, so that
    // by the time a function F is processed (its own call sites inlined),
    // every defined function F calls has *already* been processed and thus
    // contains no more calls to defined functions itself. Standard DFS
    // post-order over a DAG has exactly this property (for every edge
    // u -> v, v finishes before u), which is why the recursion check above
    // runs first. ---
    let post_order = topo_post_order(&m, n_imp_funcs, total_funcs);

    let mut dup_counts: HashMap<u32, u32> = HashMap::new();
    for f_idx in post_order {
        let Some(mut cur) = funcs.remove(&f_idx) else {
            continue;
        };
        let param_count = cur.param_types.len() as u32;
        let orig_body = std::mem::take(&mut cur.body);
        let mut new_body = Vec::with_capacity(orig_body.len());
        for op in orig_body {
            if let wasmparser::Operator::Call { function_index } = op
                && let Some(callee) = funcs.get(&function_index)
            {
                *dup_counts.entry(function_index).or_insert(0) += 1;

                // (a) Spill the arguments into fresh caller locals, in
                // reverse order (the last operand is popped first).
                let arg_base = param_count + cur.extra_locals.len() as u32;
                let arg_locals: Vec<u32> = (0..callee.param_types.len() as u32)
                    .map(|i| arg_base + i)
                    .collect();
                cur.extra_locals.extend(callee.param_types.iter().copied());
                let extra_base = param_count + cur.extra_locals.len() as u32;
                cur.extra_locals.extend(callee.extra_locals.iter().copied());

                emit_inlined(&mut new_body, callee, &arg_locals, extra_base);
                continue;
            }
            new_body.push(op);
        }
        cur.body = new_body;
        funcs.insert(f_idx, cur);
    }

    let mut report = FlattenReport::default();
    for (&idx, &count) in &dup_counts {
        if count > 1 {
            report.notes.push(format!(
                "function {idx} was inlined into {count} call sites (body duplicated {count}x)"
            ));
        }
    }

    // --- R1: ensure `_g` is imported. ---
    let existing_g = m.find_func_import("env", "_g");

    // --- R2: rebuild the type section to exactly {import types} ∪ {entry
    // type}, deduplicated. ---
    let mut new_types: Vec<wasmparser::FuncType> = Vec::new();
    let mut old_type_to_new: HashMap<u32, u32> = HashMap::new();
    for imp in &m.imports {
        if let wasmparser::TypeRef::Func(old_ty) = imp.ty
            && !old_type_to_new.contains_key(&old_ty)
        {
            let ft = m
                .types
                .get(old_ty as usize)
                .context("import has an invalid type index")?;
            let new_idx = ir::find_or_insert_type(&mut new_types, ft.params(), ft.results());
            old_type_to_new.insert(old_ty, new_idx);
        }
    }
    let entry_type_idx = ir::find_or_insert_type(
        &mut new_types,
        &[wasmparser::ValType::I32],
        &[wasmparser::ValType::I64],
    );
    let g_type_idx = if existing_g.is_none() {
        Some(ir::find_or_insert_type(
            &mut new_types,
            &[wasmparser::ValType::I32, wasmparser::ValType::I32],
            &[wasmparser::ValType::I32],
        ))
    } else {
        None
    };

    // --- Assemble the output module. ---
    let mut module = wasm_encoder::Module::new();

    let mut types_sec = wasm_encoder::TypeSection::new();
    for ty in &new_types {
        let (params, results) = ir::conv_functype(ty)?;
        types_sec.ty().function(params, results);
    }
    module.section(&types_sec);

    let mut imports_sec = wasm_encoder::ImportSection::new();
    for imp in &m.imports {
        match imp.ty {
            wasmparser::TypeRef::Func(old_ty) => {
                let new_ty = *old_type_to_new
                    .get(&old_ty)
                    .context("internal error: import type was not registered")?;
                imports_sec.import(
                    imp.module,
                    imp.name,
                    wasm_encoder::EntityType::Function(new_ty),
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
                bail!("unsupported import: tag imports are not supported");
            }
            wasmparser::TypeRef::FuncExact(_) => {
                bail!("unsupported import: exact function-reference imports are not supported");
            }
        }
    }
    if let Some(g_type_idx) = g_type_idx {
        imports_sec.import("env", "_g", wasm_encoder::EntityType::Function(g_type_idx));
    }
    module.section(&imports_sec);

    let n_new_func_imports = n_imp_funcs + u32::from(existing_g.is_none());
    let hook_new_idx = n_new_func_imports;
    let cbak_new_idx = cbak_old.map(|_| n_new_func_imports + 1);

    let mut funcs_sec = wasm_encoder::FunctionSection::new();
    funcs_sec.function(entry_type_idx);
    if cbak_old.is_some() {
        funcs_sec.function(entry_type_idx);
    }
    module.section(&funcs_sec);

    // No table section: the cleaner already dropped the table and every
    // element segment (call_indirect is banned), and flatten never adds one.

    let mut mem_sec = wasm_encoder::MemorySection::new();
    for &mem in &m.memories {
        mem_sec.memory(ir::conv_memtype(mem));
    }
    module.section(&mem_sec);

    let mut globals_sec = wasm_encoder::GlobalSection::new();
    {
        let mut remapper = ir::IndexRemapper::new(|x| x, |x| x);
        for g in &m.globals {
            let ty = ir::conv_globaltype(g.ty)?;
            let expr = ir::remap_const_expr(&g.init_expr, &mut remapper)?;
            globals_sec.global(ty, &expr);
        }
    }
    module.section(&globals_sec);

    let mut exports_sec = wasm_encoder::ExportSection::new();
    exports_sec.export("hook", wasm_encoder::ExportKind::Func, hook_new_idx);
    if let Some(idx) = cbak_new_idx {
        exports_sec.export("cbak", wasm_encoder::ExportKind::Func, idx);
    }
    module.section(&exports_sec);

    let mut code_sec = wasm_encoder::CodeSection::new();
    let hook_flat = funcs
        .get(&hook_old)
        .context("internal error: hook function state missing after flattening")?;
    code_sec.function(&encode_function(hook_flat)?);
    if let Some(cbak_old) = cbak_old {
        let cbak_flat = funcs
            .get(&cbak_old)
            .context("internal error: cbak function state missing after flattening")?;
        code_sec.function(&encode_function(cbak_flat)?);
    }
    module.section(&code_sec);

    let mut data_sec = wasm_encoder::DataSection::new();
    {
        let mut remapper = ir::IndexRemapper::new(|x| x, |x| x);
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

    Ok((module.finish(), report))
}

/// Splices an already-fully-flattened callee's body into `out`, wrapped in
/// a `block` of the callee's result type. `arg_locals[i]` is the fresh
/// caller local holding callee parameter `i`; `extra_local_base` is the
/// first fresh caller local holding callee's own (non-parameter) locals.
/// Every `return` in the callee becomes a `br` to the wrapper block, at a
/// depth computed by tracking `block`/`loop`/`if`/`end` nesting as the
/// callee's body is streamed through — the wrapper block is the outermost
/// frame of the spliced body, so a `return` at that top level (no further
/// nested construct open) becomes `br 0`. Every other `br`/`br_if`/
/// `br_table` targets a label internal to the callee's own original
/// nesting and is left unchanged, since wrapping the whole body in one more
/// block adds no *additional* enclosing scope for anything but `return`.
fn emit_inlined<'a>(
    out: &mut Vec<wasmparser::Operator<'a>>,
    callee: &FlatFunc<'a>,
    arg_locals: &[u32],
    extra_local_base: u32,
) {
    // (a) Spill arguments: last operand is popped first, so locals are set
    // in reverse parameter order.
    for &local_index in arg_locals.iter().rev() {
        out.push(wasmparser::Operator::LocalSet { local_index });
    }

    // (b) Splice the body, wrapped in a block of the callee's result type.
    let blockty = match callee.result_type {
        None => wasmparser::BlockType::Empty,
        Some(t) => wasmparser::BlockType::Type(t),
    };
    out.push(wasmparser::Operator::Block { blockty });

    let param_count = callee.param_types.len() as u32;
    let remap_local = |local_index: u32| -> u32 {
        if local_index < param_count {
            // Safe: `arg_locals` has exactly `param_count` entries (built
            // from `callee.param_types` at the call site above).
            arg_locals
                .get(local_index as usize)
                .copied()
                .unwrap_or(local_index)
        } else {
            extra_local_base + (local_index - param_count)
        }
    };

    let mut depth: u32 = 0;
    for op in &callee.body {
        match op {
            // (d) `return` -> `br <depth>`.
            wasmparser::Operator::Return => {
                out.push(wasmparser::Operator::Br {
                    relative_depth: depth,
                });
            }
            // (c) Remap local references (params -> spill locals, callee's
            // own locals -> appended caller locals).
            wasmparser::Operator::LocalGet { local_index } => {
                out.push(wasmparser::Operator::LocalGet {
                    local_index: remap_local(*local_index),
                });
            }
            wasmparser::Operator::LocalSet { local_index } => {
                out.push(wasmparser::Operator::LocalSet {
                    local_index: remap_local(*local_index),
                });
            }
            wasmparser::Operator::LocalTee { local_index } => {
                out.push(wasmparser::Operator::LocalTee {
                    local_index: remap_local(*local_index),
                });
            }
            wasmparser::Operator::Block { .. }
            | wasmparser::Operator::Loop { .. }
            | wasmparser::Operator::If { .. } => {
                depth += 1;
                out.push(op.clone());
            }
            wasmparser::Operator::End => {
                // The callee body's own trailing `End` (the one that would
                // otherwise close its function-level implicit block) closes
                // our synthetic wrapper `block` instead — no extra `end` is
                // ever appended for the wrapper. Depth bottoms out at 0
                // exactly on that final operator.
                depth = depth.saturating_sub(1);
                out.push(wasmparser::Operator::End);
            }
            // Everything else (arithmetic, `br`/`br_if`/`br_table` internal
            // to the callee's own nesting, `unreachable`, calls to imports,
            // ...) passes through unchanged.
            other => out.push(other.clone()),
        }
    }
}

/// Encodes a fully-flattened entry function's current state (locals + body)
/// into a `wasm_encoder::Function`. No index remapping is needed here: by
/// construction, a fully-flattened entry's body contains no more `call`
/// targets other than imports, and import indices never change during
/// flatten (only defined non-entry functions are dropped, and `_g` — if
/// added — is appended after every existing import).
fn encode_function(f: &FlatFunc) -> Result<wasm_encoder::Function> {
    // Run-length-encode the flat extra-locals list into (count, type) pairs
    // (not required for correctness, but keeps the encoding compact).
    let mut locals: Vec<(u32, wasm_encoder::ValType)> = Vec::new();
    for ty in &f.extra_locals {
        let ty = ir::conv_valtype(*ty)?;
        match locals.last_mut() {
            Some((count, last_ty)) if *last_ty == ty => *count += 1,
            _ => locals.push((1, ty)),
        }
    }
    let mut func = wasm_encoder::Function::new(locals);
    let mut remapper = ir::IndexRemapper::new(|x| x, |x| x);
    for op in &f.body {
        let translated = remapper
            .instruction(op.clone())
            .map_err(|e| anyhow::anyhow!("failed to translate instruction: {e}"))?;
        func.instruction(&translated);
    }
    Ok(func)
}

/// Computes a reverse topological order (callees before callers) over the
/// direct-call graph restricted to defined functions (imports are leaves
/// and never appear in the result). Standard iterative DFS post-order: for
/// a DAG, every edge `u -> v` has `v` finish before `u`, which is exactly
/// the property the flatten pass's single-pass processing loop relies on.
/// Assumes the graph is already known to be acyclic (checked by the caller
/// via [`ir::find_call_cycle`] beforehand).
fn topo_post_order(m: &ir::ParsedModule, n_imp_funcs: u32, total_funcs: u32) -> Vec<u32> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Unvisited,
        InProgress,
        Done,
    }

    let mut state = vec![State::Unvisited; total_funcs as usize];
    let get = |state: &[State], idx: u32| state.get(idx as usize).copied().unwrap_or(State::Done);
    let set = |state: &mut [State], idx: u32, s: State| {
        if let Some(slot) = state.get_mut(idx as usize) {
            *slot = s;
        }
    };
    let defined_edges = |idx: u32| -> Vec<u32> {
        match m.defined_body(idx) {
            Some(body) => match ir::scan_refs(body) {
                Ok(refs) => refs
                    .calls
                    .into_iter()
                    .filter(|&c| c >= n_imp_funcs)
                    .collect(),
                Err(_) => Vec::new(),
            },
            None => Vec::new(),
        }
    };

    let mut order = Vec::new();
    for start in n_imp_funcs..total_funcs {
        if get(&state, start) != State::Unvisited {
            continue;
        }
        let mut stack: Vec<(u32, Vec<u32>)> = vec![(start, defined_edges(start))];
        set(&mut state, start, State::InProgress);
        while let Some((node, edges)) = stack.last_mut() {
            let node = *node;
            if let Some(next) = edges.pop() {
                if get(&state, next) == State::Unvisited {
                    set(&mut state, next, State::InProgress);
                    let next_edges = defined_edges(next);
                    stack.push((next, next_edges));
                }
                // `InProgress` would mean a cycle (excluded by the caller's
                // pre-check); `Done` needs no further action.
            } else {
                set(&mut state, node, State::Done);
                order.push(node);
                stack.pop();
            }
        }
    }
    order
}
