//! Integration tests for the `--optimize` (`wasm-opt -Oz`) pipeline stage
//! (`docs/DESIGN.md` §6, PLAN M7).
//!
//! Tests that actually invoke `wasm-opt` skip themselves (rather than
//! failing) when no `wasm-opt` executable is on `PATH`, since it is an
//! optional system dependency, not a workspace toolchain pin (`mise.toml`
//! does not manage it) — see `crate::optimize`'s module doc for why it's a
//! shell-out rather than a vendored crate.
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

use std::sync::Mutex;

use hooks_build::Options;

/// Serializes every test in this file against `$WASM_OPT`: cargo runs tests
/// within one binary concurrently by default, and `$WASM_OPT` is
/// process-global state, so the negative test below (which points it at a
/// nonexistent path) could otherwise race with the positive test (which
/// needs it either unset or pointed at a real `wasm-opt`).
static WASM_OPT_ENV_LOCK: Mutex<()> = Mutex::new(());

fn wasm(src: &str) -> Vec<u8> {
    wat::parse_str(src).expect("fixture is valid wat")
}

fn optimize_opts() -> Options {
    Options {
        optimize: true,
        ..Options::default()
    }
}

/// True if a `wasm-opt` executable can be found on `PATH`, mirroring
/// `crate::optimize::find_wasm_opt`'s search (checked independently here so
/// a test skip and the pipeline's own lookup can never silently disagree
/// for a reason other than "no `wasm-opt` installed").
fn wasm_opt_on_path() -> bool {
    std::env::var("PATH")
        .ok()
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .any(|dir| dir.join("wasm-opt").is_file())
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
fn optimize_pipeline_completes_and_validates() {
    let _env_guard = WASM_OPT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    if !wasm_opt_on_path() {
        eprintln!("skipping: no `wasm-opt` on PATH (install via `brew install binaryen`)");
        return;
    }

    let input = wasm(GUARDED_LOOP_WITH_DEAD_CODE_HOOK);
    let (out, report) = hooks_build::run_pipeline(&input, &optimize_opts())
        .expect("pipeline with --optimize should complete and pass validation");

    // The full post-optimize pipeline (flatten/unnest/guard/validate/native
    // checker) must have run against wasm-opt's actual output, so `out`
    // itself must still be a SetHook-legal module.
    hooks_build::verify(&out, &Options::default())
        .expect("wasm-opt's output, after the rest of the pipeline, must independently validate");

    let summary = report
        .optimize_report
        .expect("--optimize must populate ValidationReport::optimize_report");
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

#[test]
fn optimize_missing_wasm_opt_reports_actionable_error() {
    let _env_guard = WASM_OPT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // SAFETY: `$WASM_OPT` is process-global; `WASM_OPT_ENV_LOCK` above
    // serializes this against the only other test in this file that reads
    // it, so this mutation cannot race with a concurrently-running test.
    unsafe {
        std::env::set_var(
            "WASM_OPT",
            "/definitely/not/a/real/wasm-opt/on/this/machine",
        );
    }
    let result = hooks_build::optimize(&wasm(GUARDED_LOOP_WITH_DEAD_CODE_HOOK));
    unsafe {
        std::env::remove_var("WASM_OPT");
    }

    let err = result.expect_err("a nonexistent $WASM_OPT path must not silently succeed");
    let msg = err.to_string();
    assert!(
        msg.contains("/definitely/not/a/real/wasm-opt/on/this/machine"),
        "error should name the path that failed to spawn: {msg}"
    );
}
