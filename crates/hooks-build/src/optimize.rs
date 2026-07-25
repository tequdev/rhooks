//! `wasm-opt` (Binaryen) integration, run by default *before* the cleaner
//! ([`crate::Options::optimize`]; the CLI's `build`/`clean` subcommands
//! expose `--no-optimize` to turn it off).
//!
//! **Dependency choice**: this depends on the `wasm-opt` crate (crates.io),
//! which vendors Binaryen's C++ sources and links `wasm-opt` in-process,
//! rather than shelling out to a system `wasm-opt` executable. An earlier
//! version of this module shelled out instead, to avoid the vendored
//! crate's build-time cost; that traded a slower/less portable build for a
//! capability gap (optimization silently unavailable without a separate
//! `brew install binaryen`/`apt install binaryen` step, plus `$WASM_OPT`
//! escape-hatch plumbing) that turned out to matter more than the build-time
//! cost it was avoiding — a gap that would have mattered even more once
//! optimization became the default rather than an opt-in flag. Self-
//! containment won: with the crate dependency, optimization always works,
//! on every platform `wasm-opt-sys` supports, with no separate install step
//! and no environment-variable override to document or test. The cost is
//! paid once, at build time: the `wasm-opt-sys` crate compiles Binaryen's
//! C++ sources from scratch (it does not do incremental recompilation),
//! which adds a non-trivial amount of time to a *clean* build of
//! `hooks-build` — every contributor and CI runner pays it once, then
//! `cargo`'s normal build-artifact caching means every subsequent build
//! (regardless of `--no-optimize`) is unaffected.
//!
//! Pipeline position: **before** [`crate::cleaner::clean`] (`docs/
//! DESIGN.md` §6) — deliberately ahead of the cleaner, not after it. See
//! [`crate::run_pipeline`]'s doc comment for why: running `wasm-opt` on
//! the *raw*, not-yet-cleaned module (which still carries rustc/LLVM's own
//! `memory` export) is load-bearing for correctness, not just an ordering
//! choice — Binaryen's dead-code elimination cannot otherwise tell the
//! module's linear memory is used at all, since every Hook API host
//! import reads/writes it via a raw pointer argument rather than a wasm
//! `memory.load`/`memory.store` opcode, and will delete the memory section
//! (and its data segment) outright once nothing visibly references it.
//! [`optimize`] restricts the optimizer to the WebAssembly MVP feature set
//! explicitly (via [`wasm_opt::OptimizationOptions::mvp_features_only`])
//! rather than relying on Binaryen's default feature baseline (which
//! enables sign-extension ops and mutable-globals import/export beyond
//! MVP) — matching the MVP-only shape `wasm32v1-none`/the vendored guard
//! checker expect; this holds regardless of whether a `target_features`
//! custom section is present on the (here, not-yet-cleaned) input.
//! `wasm-opt`'s output is **not** trusted blindly: every later stage
//! (cleaner, flatten, unnest, guard pass/verify, Rust validator, vendored
//! upstream checker) still runs against it exactly as it would against
//! un-optimized input, so any shape `-Oz` produces that those stages reject
//! is still caught before it ever reaches disk.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use wasm_opt::OptimizationOptions;

/// Before/after statistics for a `wasm-opt` run, attached to
/// [`crate::ValidationReport::optimize_report`].
#[derive(Debug, Clone, Copy)]
pub struct OptimizeReport {
    /// Size, in bytes, of the cleaned module before `wasm-opt` ran.
    pub before_bytes: usize,
    /// Size, in bytes, of the final module after `wasm-opt` (and every
    /// later pipeline stage) ran.
    pub after_bytes: usize,
    /// Estimated SetHook fee before optimization.
    pub before_fee: crate::FeeEstimate,
    /// Estimated SetHook fee after optimization.
    pub after_fee: crate::FeeEstimate,
    /// The vendored upstream guard checker's worst-case instruction counts
    /// for the cleaned-but-not-yet-optimized module, if they could be
    /// computed (API version 0 only; `None` if flatten/unnest/the checker
    /// itself could not run on the pre-optimize shape — this is purely
    /// informational and never blocks the build).
    pub before_guard: Option<crate::GuardVerdict>,
    /// The vendored upstream guard checker's worst-case instruction counts
    /// for the final, optimized module. Populated from the same
    /// [`crate::ValidationReport::guard_verdict`] the rest of the pipeline
    /// computes, so unlike `before_guard` it is never independently
    /// recomputed.
    pub after_guard: Option<crate::GuardVerdict>,
}

/// Runs `wasm-opt -Oz` (MVP feature set only) over `wasm`, returning the
/// optimized bytes.
///
/// Errors with `wasm-opt`'s own diagnostic if it fails to parse, optimize,
/// or validate the input or output module.
pub fn optimize(wasm: &[u8]) -> Result<Vec<u8>> {
    let unique = format!(
        "hooks-build-optimize-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default(),
    );
    let dir = std::env::temp_dir();
    let in_path = dir.join(format!("{unique}-in.wasm"));
    let out_path = dir.join(format!("{unique}-out.wasm"));

    let result = run_wasm_opt(wasm, &in_path, &out_path);

    let _ = std::fs::remove_file(&in_path);
    let _ = std::fs::remove_file(&out_path);

    result
}

fn run_wasm_opt(wasm: &[u8], in_path: &PathBuf, out_path: &PathBuf) -> Result<Vec<u8>> {
    std::fs::write(in_path, wasm)
        .with_context(|| format!("writing wasm-opt input to {}", in_path.display()))?;

    // `-Oz`: optimize_level 2, shrink_level 2 (Binaryen's most aggressive
    // size-optimization preset), restricted to the WebAssembly MVP feature
    // set — see this module's doc comment.
    OptimizationOptions::new_optimize_for_size_aggressively()
        .mvp_features_only()
        .run(in_path, out_path)
        .context("wasm-opt (-Oz) failed")?;

    std::fs::read(out_path)
        .with_context(|| format!("reading wasm-opt output {}", out_path.display()))
}
