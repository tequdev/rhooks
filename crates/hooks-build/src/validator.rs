//! The SetHook validator: `docs/DESIGN.md` §6.4.
//!
//! Applies the full SetHook-derived hard-error and warning rule set to a
//! module's *final* bytes. Used both as the last stage of `build`/`clean`
//! (after cleaning and the optional guard pass) and as the entirety of
//! `check`, which runs it against arbitrary wasm (including C-built hooks).

use anyhow::{Result, bail};

use crate::guard::{find_g_index, guard_hint, scan_function_loops};
use crate::ir;
use crate::{ApiVersion, Options};

/// The maximum size, in bytes, of a SetHook-legal wasm binary.
pub const MAX_SIZE: usize = 65_535;

/// Sizes at or above this many bytes trigger a "getting close to the limit"
/// warning.
pub const SIZE_WARNING_THRESHOLD: usize = 56 * 1024;

/// The maximum `block`/`loop`/`if` nesting depth a SetHook-legal
/// api-version-0 module's function bodies may reach (`Guard.h`
/// `NESTING_LIMIT` under `GuardRuleDepth32`; see `docs/DESIGN.md` §6.2c).
pub const MAX_NESTING_DEPTH: u32 = 32;

/// Nesting depths at or above this level trigger an "approaching the limit"
/// warning (api-version 0 only).
pub const NESTING_DEPTH_WARNING_THRESHOLD: u32 = 28;

/// The maximum number of WASM pages (64 KiB each) a Gas-type (API version 1)
/// hook's exported memory may declare, for both the minimum and (if present)
/// maximum limits. Matches `hook_api::max_memory_pages` (`Enum.h`) as enforced
/// by `GasValidator.cpp`'s `validateExportSection()` — a rule with no
/// api-version-0 counterpart (the guard checker does not check memory page
/// counts).
pub const GAS_MAX_MEMORY_PAGES: u64 = 8;

/// The result of a successful validation: any non-fatal warnings found.
#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    /// Human-readable warning messages.
    pub warnings: Vec<String>,
    /// True if the module exceeded [`MAX_SIZE`] but was allowed through by
    /// `opts.allow_oversize` (only ever set outside of `check`).
    pub oversize_allowed: bool,
    /// The worst-case instruction counts reported by the vendored upstream
    /// guard checker (`docs/DESIGN.md` §6.5), when it ran and accepted the
    /// module. Only ever set for API version 0, by
    /// [`crate::verify`]/[`crate::run_pipeline`] — [`validate`] itself
    /// (the pure-Rust pass) never populates this field.
    pub guard_verdict: Option<crate::GuardVerdict>,
    /// The maximum `block`/`loop`/`if` nesting depth reached by any defined
    /// function in the module (0 if the module defines no such construct).
    /// Computed for every api version so `build`/`check` can always print
    /// it; only api-version 0 hard-errors/warns on it (`docs/DESIGN.md`
    /// §6.2c/§6.4).
    pub max_nesting_depth: u32,
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
            // Gas-type (API version 1) hooks may export other `__`-prefixed
            // functions (runtime-support symbols) — `GasValidator.cpp`'s
            // `validateExportSection()` explicitly skips these
            // (`nameStr.starts_with("__")`) rather than rejecting them. Our
            // own `clean`/`build` pipeline never emits any (the cleaner
            // restricts exports to `hook`/`cbak` unconditionally), so this
            // only matters for `check` against externally-built wasm.
            (name, wasmparser::ExternalKind::Func)
                if opts.api_version == ApiVersion::V1 && name.starts_with("__") => {}
            // Gas-type hooks may export their memory — `GasValidator.cpp`
            // allows it, enforcing only the page-count limit. API-version-0
            // modules have no such allowance: our cleaner always strips a
            // memory export, and an exported memory reaching `validate()`
            // for a v0 module is treated as unexpected (see
            // `validator_rejects_extra_export`).
            (_, wasmparser::ExternalKind::Memory) if opts.api_version == ApiVersion::V1 => {
                check_gas_memory_export(&m, e.index, &mut errors);
            }
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
    if let Some(cycle) = ir::find_call_cycle(&m) {
        errors.push(format!(
            "recursive call cycle detected among functions: {cycle:?} (recursion is not allowed)"
        ));
    }

    // --- Guards, R1, R2 (API version 0 only; `docs/DESIGN.md` §6.2b/§6.4). ---
    if opts.api_version == ApiVersion::V0 {
        let g_index = find_g_index(&m);

        // R1: every api-version-0 module must import `_g`, even if it has no
        // loop at all — the vendored upstream checker enforces this
        // unconditionally (discovered running it against phase-4 artifacts
        // that the pure-Rust validator had wrongly accepted).
        if g_index.is_none() {
            errors.push(
                "module does not import `_g` (env::_g, type (i32,i32)->i32) — required for \
                 every api-version-0 module, even without loops (R1)"
                    .to_string(),
            );
        }

        // R2: every type-section entry must be the type of an import or the
        // `(i32) -> i64` entry-point type. A defined helper function with any
        // other signature (notably compiler_builtins memset/memcpy/bcmp,
        // `(i32,i32,i32) -> i32`) makes the whole module invalid to the
        // upstream checker; the flatten pass (§6.2b) is what makes this hold
        // for api-version-0 modules built through `hooks-build`.
        let entry_ty = (
            [wasmparser::ValType::I32].as_slice(),
            [wasmparser::ValType::I64].as_slice(),
        );
        let import_shapes: std::collections::HashSet<(
            &[wasmparser::ValType],
            &[wasmparser::ValType],
        )> = m
            .imports
            .iter()
            .filter_map(|imp| match imp.ty {
                wasmparser::TypeRef::Func(idx) => m.types.get(idx as usize),
                _ => None,
            })
            .map(|ty| (ty.params(), ty.results()))
            .collect();
        for (i, ty) in m.types.iter().enumerate() {
            let shape = (ty.params(), ty.results());
            if shape != entry_ty && !import_shapes.contains(&shape) {
                errors.push(format!(
                    "type {i} (`({:?}) -> {:?}`) is neither an import's type nor the entry-point \
                     type `(i32) -> i64` (R2) — this is only reachable if a defined helper \
                     function was left un-inlined",
                    ty.params(),
                    ty.results()
                ));
            }
        }

        for (i, body) in m.code.iter().enumerate() {
            let func_idx = m.num_imported_funcs() + i as u32;
            match scan_function_loops(body, g_index) {
                Ok(sites) => {
                    for site in sites.iter().filter(|s| !s.guarded) {
                        let mut msg = format!(
                            "function {func_idx}, offset {}: `loop` is missing a guard (`i32.const; i32.const; call $_g`)",
                            site.offset
                        );
                        if let Some(hint) = guard_hint(site.guess) {
                            msg.push_str(" — ");
                            msg.push_str(hint);
                        }
                        errors.push(msg);
                    }
                }
                Err(e) => errors.push(format!(
                    "function {func_idx}: failed to scan for guards: {e}"
                )),
            }
        }
    } else {
        // --- Gas-type (API version 1): `_g` must not be imported at all.
        // `GasValidator.cpp`'s `validateImportSection()` explicitly rejects
        // it ("Gas-type hooks cannot import _g (guard) function") — guard
        // calls have no meaning once loop iteration is bounded by gas
        // metering instead of a static instruction-count analysis.
        if find_g_index(&m).is_some() {
            errors.push(
                "module imports `_g` (env::_g) — Gas-type (API version 1) hooks must not \
                 import the guard function; guard calls are meaningless under gas metering"
                    .to_string(),
            );
        }
    }

    // --- Nesting depth: computed for every defined function, for every api
    // version (so `build`/`check` can always print the module's overall
    // max), but only api-version 0 hard-errors/warns on it — `Guard.h`
    // `NESTING_LIMIT` under `GuardRuleDepth32` is specifically a guard-type
    // (api-version 0) rule; see `docs/DESIGN.md` §6.2c/§6.4. ---
    let mut max_overall_depth: u32 = 0;
    for (i, body) in m.code.iter().enumerate() {
        let func_idx = m.num_imported_funcs() + i as u32;
        match ir::max_nesting_depth(body) {
            Ok(depth) => {
                max_overall_depth = max_overall_depth.max(depth);
                if opts.api_version == ApiVersion::V0 {
                    if depth > MAX_NESTING_DEPTH {
                        errors.push(format!(
                            "function {func_idx}: block/loop/if nesting depth is {depth}, \
                             exceeding the {MAX_NESTING_DEPTH}-level limit (`Guard.h` \
                             `NESTING_LIMIT` under `GuardRuleDepth32`)"
                        ));
                    } else if depth >= NESTING_DEPTH_WARNING_THRESHOLD {
                        warnings.push(format!(
                            "function {func_idx}: block/loop/if nesting depth is {depth}, \
                             approaching the {MAX_NESTING_DEPTH}-level limit"
                        ));
                    }
                }
            }
            Err(e) => errors.push(format!(
                "function {func_idx}: failed to compute nesting depth: {e}"
            )),
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
        guard_verdict: None,
        max_nesting_depth: max_overall_depth,
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

/// Validates a Gas-type (API version 1) hook's exported memory against
/// [`GAS_MAX_MEMORY_PAGES`]: both the minimum and (if present) maximum page
/// counts must not exceed the limit. Mirrors `GasValidator.cpp`'s
/// `validateExportSection()`.
fn check_gas_memory_export(m: &ir::ParsedModule, mem_index: u32, errors: &mut Vec<String>) {
    let Some(mem) = m.memories.get(mem_index as usize) else {
        errors.push(format!(
            "exported memory {mem_index} has an invalid memory index"
        ));
        return;
    };
    if mem.initial > GAS_MAX_MEMORY_PAGES {
        errors.push(format!(
            "Gas-type hook exported memory minimum pages ({}) exceed limit of \
             {GAS_MAX_MEMORY_PAGES}",
            mem.initial
        ));
    }
    if let Some(max) = mem.maximum
        && max > GAS_MAX_MEMORY_PAGES
    {
        errors.push(format!(
            "Gas-type hook exported memory maximum pages ({max}) exceed limit of \
             {GAS_MAX_MEMORY_PAGES}"
        ));
    }
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
