//! Fuzz target for the `hooks-build` pipeline (`docs/DESIGN.md` §6):
//! `wasm-smith` generates a module under a `Config` approximating a
//! SetHook-legal module's constraints (single memory, no floats/SIMD/
//! GC/exceptions/threads/tail-calls/reference-types, no imports so an
//! executable case can be instantiated with an empty `wasmi` linker), then:
//!
//! (a) `hooks_build::clean` must never panic on it,
//! (b) a successfully cleaned/flattened/unnested module must still be
//!     valid wasm per `wasmparser`,
//! (c) when the pre-pipeline module is itself instantiable (no traps at
//!     module start, which `disallow_traps` mostly ensures), the "hook"
//!     entry point's observable behavior (return value) must be unchanged
//!     by clean -> flatten -> unnest, mirroring the differential style of
//!     `crates/hooks-build/tests/flatten_differential.rs` /
//!     `unnest_differential.rs`.
//!
//! `wasm-smith`'s raw output essentially never happens to export a function
//! named `hook` with the required `(i32) -> i64` signature on its own, so
//! this target manufactures the entry point itself: it scans the generated
//! module's defined functions for one with that exact signature and
//! rewrites the export section to export it as `hook` (dropping whatever
//! exports `wasm-smith` produced — `hooks-build`'s cleaner strips all
//! exports but `hook`/`cbak` anyway, so this loses no coverage). A module
//! with no such function fails hooks-build's own entry-condition check
//! (`clean` requires a `hook` export); that is an expected, out-of-scope
//! early skip.
#![no_main]

use arbitrary::Unstructured;
use hooks_build::Options;
use libfuzzer_sys::fuzz_target;
use wasm_smith::{Config, Module};
use wasmi::{Engine, Linker, Module as WasmiModule, Store};

/// A `wasm-smith` config approximating what a SetHook module may contain
/// (`docs/DESIGN.md` §2, §6): MVP instruction/type set only, a single
/// memory, no tables, and — deliberately — no imports at all, so any module
/// this produces is self-contained and instantiable with an empty `wasmi`
/// linker (real hooks import `env` functions, but exercising that would
/// require stubbing all 74 of them; the cleaner/flatten/unnest passes under
/// test here don't care whether a call target is an import or not). Kept
/// small so the smoke run stays fast.
fn hook_like_config() -> Config {
    Config {
        allow_floats: false,
        simd_enabled: false,
        relaxed_simd_enabled: false,
        exceptions_enabled: false,
        gc_enabled: false,
        custom_descriptors_enabled: false,
        threads_enabled: false,
        shared_everything_threads_enabled: false,
        tail_call_enabled: false,
        reference_types_enabled: false,
        memory64_enabled: false,
        custom_page_sizes_enabled: false,
        wide_arithmetic_enabled: false,
        bulk_memory_enabled: false,
        generate_custom_sections: false,
        allow_start_export: false,
        disallow_traps: true,
        // Real hooks run in a tiny linear memory (a handful of pages at
        // most); `wasm-smith`'s default (`u32::MAX + 1`, i.e. up to 4 GiB)
        // lets `wasmi`'s eager instantiation-time allocation OOM the fuzzer
        // on a memory type whose declared minimum is merely large, not even
        // actually touched — nothing to do with `hooks-build` itself. 16
        // pages (1 MiB) is generous for a fixture and keeps `run_hook`'s
        // `wasmi` instantiation cheap.
        max_memory32_bytes: 16 * 65536,
        max_memories: 1,
        min_memories: 1,
        max_tables: 0,
        min_tables: 0,
        max_tags: 0,
        min_tags: 0,
        max_imports: 0,
        min_imports: 0,
        max_funcs: 24,
        min_funcs: 1,
        max_instructions: 200,
        max_type_size: 40,
        ..Config::default()
    }
}

/// Finds the defined-function index of the first function whose signature
/// is exactly `(i32) -> i64` — the only Hook entry-point signature
/// (`check_entry_signature` in `crates/hooks-build/src/cleaner.rs`). Returns
/// `None` on any parse error or if no such function exists (both treated as
/// an "entry condition not met" skip by the caller). `hook_like_config`
/// forces `max_imports: 0`, so a defined-function's position in the
/// `FunctionSection` is already its absolute function index.
fn entry_candidate(wasm: &[u8]) -> Option<u32> {
    let mut types: Vec<wasmparser::FuncType> = Vec::new();
    let mut defined_func_types: Vec<u32> = Vec::new();

    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        match payload.ok()? {
            wasmparser::Payload::TypeSection(reader) => {
                for rec_group in reader {
                    for sub in rec_group.ok()?.into_types() {
                        if let wasmparser::CompositeInnerType::Func(ft) = sub.composite_type.inner {
                            types.push(ft);
                        }
                    }
                }
            }
            wasmparser::Payload::FunctionSection(reader) => {
                for f in reader {
                    defined_func_types.push(f.ok()?);
                }
            }
            _ => {}
        }
    }

    defined_func_types
        .iter()
        .enumerate()
        .find_map(|(idx, &ty_idx)| {
            let ty = types.get(ty_idx as usize)?;
            if ty.params() == [wasmparser::ValType::I32]
                && ty.results() == [wasmparser::ValType::I64]
            {
                Some(idx as u32)
            } else {
                None
            }
        })
}

/// Rewrites `wasm`'s export section to contain exactly one export, `"hook"`
/// pointing at `func_index`, copying every other section through
/// byte-for-byte via `wasmparser::Payload::as_section`'s raw ranges. Returns
/// `None` on any parse error (again, an upstream skip — this only ever
/// receives `wasm-smith`'s own output, so a parse error here would mean a
/// `wasm-smith` bug outside this crate's scope, not a `hooks-build` one).
fn with_hook_export(wasm: &[u8], func_index: u32) -> Option<Vec<u8>> {
    let mut module = wasm_encoder::Module::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        let payload = payload.ok()?;
        if matches!(payload, wasmparser::Payload::ExportSection(_)) {
            let mut exports = wasm_encoder::ExportSection::new();
            exports.export("hook", wasm_encoder::ExportKind::Func, func_index);
            module.section(&exports);
            continue;
        }
        if let Some((id, range)) = payload.as_section() {
            module.section(&wasm_encoder::RawSection {
                id,
                data: wasm.get(range)?,
            });
        }
    }
    Some(module.finish())
}

/// Instantiates `wasm` (which must already export `"hook": (i32) -> i64`
/// and import nothing) with an empty `wasmi` linker and calls
/// `hook(param)`. Returns `None` if instantiation or the call traps —
/// `disallow_traps` makes an ordinary trap rare but not impossible (e.g.
/// stack exhaustion from deep recursion), and fuel exhaustion (see below)
/// is the expected outcome for a hook body containing an infinite loop —
/// in every such case this fuzz target just skips the differential (c)
/// comparison for that inconclusive run.
///
/// `wasm-smith` can (and, empirically, sometimes did while developing this
/// target — nothing to do with `hooks-build`) generate a `hook` body with
/// an unconditionally infinite loop; a bare `wasmi` call has no built-in
/// time limit and would hang the fuzzer forever. Fuel metering
/// (`Config::consume_fuel`) bounds every call deterministically: once
/// `FUEL_BUDGET` is exhausted, `wasmi` raises an ordinary
/// `TrapCode::OutOfFuel` trap instead of looping forever, which this
/// function treats exactly like any other trap (`None`, `.ok()?`).
///
/// `hook_like_config` forbids `wasm-smith` from generating any imports at
/// all, so the *pre*-pipeline module is always self-contained — but
/// `hooks_build::flatten` unconditionally adds an `env::_g` import to its
/// *output* even when the source has zero loops (`docs/DESIGN.md` §6.2b,
/// rule R1; `crates/hooks-build/tests/flatten_differential.rs`'s `run()`
/// stubs the exact same import for the exact same reason). The linker must
/// define it too, or every post-flatten/-unnest instantiation fails for a
/// reason that has nothing to do with this target's actual (a)/(b)/(c)
/// properties.
const FUEL_BUDGET: u64 = 1_000_000;

fn run_hook(wasm: &[u8], param: i32) -> Option<i64> {
    let mut config = wasmi::Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module = WasmiModule::new(&engine, wasm).ok()?;
    let mut store = Store::new(&engine, ());
    store.set_fuel(FUEL_BUDGET).ok()?;
    let mut linker = <Linker<()>>::new(&engine);
    linker
        .func_wrap("env", "_g", |_a: i32, _b: i32| -> i32 { 1 })
        .ok()?;
    let instance = linker
        .instantiate(&mut store, &module)
        .ok()?
        .start(&mut store)
        .ok()?;
    let entry = instance.get_typed_func::<i32, i64>(&store, "hook").ok()?;
    entry.call(&mut store, param).ok()
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok(module) = Module::new(hook_like_config(), &mut u) else {
        return;
    };
    let pre = module.to_bytes();

    let Some(func_index) = entry_candidate(&pre) else {
        // No function with the required (i32) -> i64 signature at all:
        // the entry condition can't be satisfied, skip (per this target's
        // doc comment).
        return;
    };
    let Some(pre) = with_hook_export(&pre, func_index) else {
        return;
    };

    // (a) panic-freedom + (b) wasmparser validity of hooks-build's own
    // output, for whichever stages a bare `wasm-smith` module (no `_g`
    // import, arbitrary call graph) can reach.
    let Ok(cleaned) = hooks_build::clean(&pre, &Options::default()) else {
        // Missing/mis-shaped `hook` export, or some other entry-condition
        // failure `clean` itself reports as an `Err` — an expected skip,
        // not a finding.
        return;
    };
    wasmparser::validate(&cleaned).expect("hooks_build::clean must emit valid wasm");

    let Ok((flattened, _report)) = hooks_build::flatten(&cleaned) else {
        return;
    };
    wasmparser::validate(&flattened).expect("hooks_build::flatten must emit valid wasm");

    let Ok((unnested, _report)) = hooks_build::unnest(&flattened) else {
        return;
    };
    wasmparser::validate(&unnested).expect("hooks_build::unnest must emit valid wasm");

    // (c) differential: when the *pre*-pipeline module itself runs cleanly
    // (no trap) for a given `param`, the fully-processed module must return
    // the exact same value for that `param` — clean/flatten/unnest must
    // never change a hook's observable behavior.
    for param in [0i32, 1, -1, i32::MIN, i32::MAX] {
        if let Some(pre_result) = run_hook(&pre, param) {
            let post_result = run_hook(&unnested, param).unwrap_or_else(|| {
                panic!(
                    "hook({param}) succeeded before the pipeline but trapped after \
                     clean -> flatten -> unnest"
                )
            });
            assert_eq!(
                pre_result, post_result,
                "hook({param}) return value diverged after clean -> flatten -> unnest"
            );
        }
    }
});
