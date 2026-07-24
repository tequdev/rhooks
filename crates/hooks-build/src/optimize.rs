//! Optional `wasm-opt` (Binaryen) integration (`--optimize`).
//!
//! **Dependency choice**: this shells out to a system `wasm-opt` executable
//! rather than depending on the `wasm-opt`/`binaryen` crates (which vendor
//! and build the whole Binaryen C++ tree from source). That crate adds
//! several minutes to a clean build of `hooks-build` itself (a one-time cost
//! for every contributor and every CI run, paid whether or not `--optimize`
//! is ever used) and a large, platform-sensitive C++ build dependency, for a
//! feature this document already scopes as optional and off by default. A
//! shell-out costs nothing when `--optimize` is not passed, keeps
//! `hooks-build`'s own build fast and portable, and Binaryen's `wasm-opt` CLI
//! is a stable, widely-packaged tool (`brew install binaryen`,
//! `apt install binaryen`, or a prebuilt release archive) — so the trade is
//! a slightly worse first-run experience (one extra install step, once) for
//! a meaningfully better one on every other build. The cost is a clear,
//! actionable error (see [`find_wasm_opt`]) when it's missing, rather than a
//! silent capability gap.
//!
//! Pipeline position: after [`crate::cleaner::clean`], before
//! [`crate::flatten::flatten`] (`docs/DESIGN.md` §6). Running it on the
//! already-cleaned module means custom sections (including any
//! `target_features` section LLVM emits) are already gone, so `wasm-opt`
//! falls back to its conservative MVP-only default feature set instead of
//! auto-detecting proposals from that section — matching the MVP-only shape
//! `wasm32v1-none`/the vendored guard checker expect. `wasm-opt`'s output is
//! **not** trusted blindly: every later stage (flatten, unnest, guard
//! pass/verify, Rust validator, vendored upstream checker) still runs
//! against it exactly as it would against un-optimized input, so any shape
//! `-Oz` produces that those stages reject is still caught before it ever
//! reaches disk.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

/// Before/after statistics for an `--optimize` run, attached to
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

/// Runs `wasm-opt -Oz` over `wasm`, returning the optimized bytes.
///
/// Errors with a clear, actionable message (see [`find_wasm_opt`]) if
/// `wasm-opt` is not on `PATH`, or with `wasm-opt`'s own stderr if it runs
/// but rejects/fails on the input.
pub fn optimize(wasm: &[u8]) -> Result<Vec<u8>> {
    let wasm_opt = find_wasm_opt()?;

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

    let result = run_wasm_opt(&wasm_opt, wasm, &in_path, &out_path);

    let _ = std::fs::remove_file(&in_path);
    let _ = std::fs::remove_file(&out_path);

    result
}

fn run_wasm_opt(
    wasm_opt: &PathBuf,
    wasm: &[u8],
    in_path: &PathBuf,
    out_path: &PathBuf,
) -> Result<Vec<u8>> {
    std::fs::write(in_path, wasm)
        .with_context(|| format!("writing wasm-opt input to {}", in_path.display()))?;

    let output = Command::new(wasm_opt)
        .arg("-Oz")
        .arg(in_path)
        .arg("-o")
        .arg(out_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("failed to spawn `{}`", wasm_opt.display()))?;

    if !output.status.success() {
        bail!(
            "wasm-opt failed ({}):\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    std::fs::read(out_path)
        .with_context(|| format!("reading wasm-opt output {}", out_path.display()))
}

/// Locates a `wasm-opt` executable: `$WASM_OPT` if set, otherwise the first
/// `wasm-opt` found on `PATH`.
fn find_wasm_opt() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("WASM_OPT") {
        return Ok(PathBuf::from(p));
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("wasm-opt");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    bail!(
        "`--optimize` requires the `wasm-opt` executable (from the Binaryen toolkit) on PATH, \
         but it was not found.\n\nInstall it, then retry:\n  \
         macOS (Homebrew): brew install binaryen\n  \
         Debian/Ubuntu:    apt install binaryen\n  \
         or download a release from https://github.com/WebAssembly/binaryen/releases\n\n\
         Alternatively, set $WASM_OPT to the executable's path, or drop --optimize to build \
         without it."
    )
}
