//! Integration tests for the `wasm-opt -Oz` pipeline stage, which runs by
//! default (`Options::optimize` defaults to `true`; the CLI's `build`/
//! `clean` subcommands expose `--no-optimize` to opt out) (`docs/DESIGN.md`
//! §6, PLAN M7).
//!
//! `wasm-opt` is vendored in-process via the `wasm-opt` crate (see
//! `crate::optimize`'s module doc), not a system executable, so these tests
//! always run — no `PATH` probing or skip-if-missing logic needed.
//!
//! Test code is exempt from the workspace's panic-freedom lints (per
//! `docs/DESIGN.md` §8): `unwrap`/`expect`/`panic!` on a known-good fixture
//! is the normal, idiomatic way to assert behavior in a test.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use hooks_build::Options;

fn wasm(src: &str) -> Vec<u8> {
    wat::parse_str(src).expect("fixture is valid wat")
}

fn no_optimize_opts() -> Options {
    Options {
        optimize: false,
        ..Options::default()
    }
}

/// A guarded-loop hook (clears the vendored checker's unconditional `_g`
/// import requirement) with an unused, fully-constant-foldable computation
/// stuffed into the loop body — dead weight `wasm-opt -Oz` is expected to
/// strip, giving the size/instruction-count comparison something real to
/// show.
const GUARDED_LOOP_WITH_DEAD_CODE_HOOK: &str = r#"
(module
  (import "env" "_g" (func $g (param i32 i32) (result i32)))
  (func $hook (param i32) (result i64)
    (local $i i32)
    (local $junk i32)
    (loop $l
      (call $g (i32.const 1) (i32.const 10))
      drop
      ;; Dead computation: $junk is never read after being written, so a
      ;; competent DCE pass (wasm-opt -Oz) removes all of this.
      (local.set $junk
        (i32.add
          (i32.mul (i32.const 6) (i32.const 7))
          (i32.sub (i32.const 100) (i32.const 58))))
      (local.set $junk (i32.mul (local.get $junk) (i32.const 0)))
      (local.set $junk (i32.add (local.get $junk) (i32.const 0)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br_if $l (i32.lt_u (local.get $i) (i32.const 10))))
    (i64.const 0))
  (export "hook" (func $hook)))
"#;

#[test]
fn optimize_runs_by_default_and_pipeline_validates() {
    let input = wasm(GUARDED_LOOP_WITH_DEAD_CODE_HOOK);
    // `Options::default()` is the point under test here: optimization must
    // run without the caller opting in to anything.
    let (out, report) = hooks_build::run_pipeline(&input, &Options::default())
        .expect("the default pipeline (optimize on) should complete and pass validation");

    // The full post-optimize pipeline (flatten/unnest/guard/validate/native
    // checker) must have run against wasm-opt's actual output, so `out`
    // itself must still be a SetHook-legal module.
    hooks_build::verify(&out, &Options::default())
        .expect("wasm-opt's output, after the rest of the pipeline, must independently validate");

    let summary = report
        .optimize_report
        .expect("optimize running by default must populate ValidationReport::optimize_report");
    assert_eq!(
        summary.after_bytes,
        out.len(),
        "reported after_bytes must match the actual final output size"
    );
    assert!(
        summary.before_bytes > 0 && summary.after_bytes > 0,
        "before/after sizes should both be recorded: {summary:?}"
    );
    assert!(
        summary.after_bytes <= summary.before_bytes,
        "wasm-opt -Oz should not grow this dead-code-laden fixture: before={} after={}",
        summary.before_bytes,
        summary.after_bytes
    );

    let before_guard = summary
        .before_guard
        .expect("api-version-0 fixture should yield a pre-optimize guard verdict");
    let after_guard = summary
        .after_guard
        .expect("api-version-0 success should carry a post-optimize guard verdict");
    assert!(
        after_guard.hook_cost <= before_guard.hook_cost,
        "dead-code elimination should not increase hook() worst-case instructions: \
         before={before_guard:?} after={after_guard:?}"
    );
}

/// Shaped like `rustc`'s own raw `wasm32v1-none` cdylib output, not like
/// the cleaned/final module: `memory` is exported (every real build does
/// this; the cleaner strips it), there's a `data` segment, and `hook()`
/// touches that memory only *indirectly* — by passing a pointer/length
/// into it as plain `i32` arguments to a host import (`trace`, per
/// `crates/hooks-build/src/whitelist.rs`) — never via an actual
/// `memory.load`/`memory.store` opcode.
///
/// This is a regression fixture for a real bug: an earlier pipeline
/// ordering ran `wasm-opt -Oz` *after* the cleaner, which had already
/// stripped this `memory` export. With no export, import, or opcode
/// visibly referencing the memory, Binaryen's dead-code elimination
/// concluded the whole memory section (and its data segment) was unused
/// and deleted it outright — a module that still passed every static
/// check in this crate (nothing in the code references a now-missing
/// memory via an opcode) but silently read garbage/trapped at runtime on
/// a live node, since the host call still received offsets into memory
/// that no longer existed. Caught via live e2e (`docs/E2E-TESTING.md`),
/// not by any static check: 2 of the 10 example hooks failed this way on
/// a real node under a default (optimize-on) build while `hooks-build
/// check` reported them clean.
const RAW_MEMORY_REFERENCED_ONLY_VIA_HOST_CALL_POINTER_HOOK: &str = r#"
(module
  (import "env" "trace" (func $trace (param i32 i32 i32 i32 i32) (result i64)))
  (import "env" "accept" (func $accept (param i32 i32 i64) (result i64)))
  (func $hook (param i32) (result i64)
    (drop (call $trace (i32.const 0) (i32.const 5) (i32.const 1) (i32.const 0) (i32.const 0)))
    (drop (call $accept (i32.const 0) (i32.const 0) (i64.const 0)))
    unreachable)
  (memory (;0;) 1)
  (export "memory" (memory 0))
  (export "hook" (func $hook))
  (data (i32.const 0) "hello"))
"#;

#[test]
fn optimize_does_not_delete_memory_referenced_only_via_host_call_pointers() {
    let input = wasm(RAW_MEMORY_REFERENCED_ONLY_VIA_HOST_CALL_POINTER_HOOK);
    let (out, _report) = hooks_build::run_pipeline(&input, &Options::default()).expect(
        "optimize (on by default) must not corrupt a hook whose memory is referenced \
         only via host-call pointer arguments",
    );

    let mut has_memory = false;
    let mut data_bytes: Vec<u8> = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(&out) {
        match payload.expect("optimized+cleaned output must still parse as valid wasm") {
            wasmparser::Payload::MemorySection(reader) => has_memory = reader.count() > 0,
            wasmparser::Payload::DataSection(reader) => {
                for data in reader {
                    data_bytes.extend_from_slice(data.expect("data segment parses").data);
                }
            }
            _ => {}
        }
    }
    assert!(
        has_memory,
        "wasm-opt must not delete the module's memory section just because no opcode \
         (as opposed to a host-call argument) references it"
    );
    assert_eq!(
        data_bytes, b"hello",
        "wasm-opt must not delete a data segment referenced only via a host-call pointer"
    );
}

#[test]
fn no_optimize_skips_wasm_opt_and_pipeline_still_validates() {
    let input = wasm(GUARDED_LOOP_WITH_DEAD_CODE_HOOK);
    let (out, report) = hooks_build::run_pipeline(&input, &no_optimize_opts())
        .expect("--no-optimize (Options::optimize = false) should still complete and validate");

    hooks_build::verify(&out, &Options::default())
        .expect("un-optimized output must independently validate too");

    assert!(
        report.optimize_report.is_none(),
        "--no-optimize must leave ValidationReport::optimize_report unset: {:?}",
        report.optimize_report
    );
    assert!(
        report.guard_verdict.is_some(),
        "api-version-0 success should still carry guard-checker instruction counts \
         without optimization"
    );
}
