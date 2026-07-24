//! `hooks-build` — a CLI and library that turns a Rust crate targeting
//! `wasm32v1-none` into a SetHook-valid WASM binary.
//!
//! The pipeline (see `docs/DESIGN.md` §6) is a handful of independently-
//! testable, byte-in/byte-out stages:
//!
//! 1. [`cleaner::clean`] — drops custom sections, restricts exports to
//!    `hook`/`cbak`, garbage-collects unreachable functions/globals, trims
//!    trailing zero bytes off active data segments, and re-encodes the
//!    module with a whole-module index remap.
//! 2. [`flatten::flatten`] — (API version 0 only) inlines every defined
//!    non-entry function into its callers, drops them, and rebuilds the
//!    type section to exactly {import types} ∪ {entry type} — ensuring
//!    `_g` is imported along the way. See `docs/DESIGN.md` §6.2b for why:
//!    the vendored upstream checker requires both unconditionally (R1, R2).
//! 3. [`unnest::unnest`] — (API version 0 only) collapses the "error
//!    ladder" of nested blocks LLVM emits for diverging early exits, which
//!    would otherwise push nesting depth past the vendored checker's
//!    32-level limit for any hook with more than a few checks. See
//!    `docs/DESIGN.md` §6.2c.
//! 4. [`guard::auto_guard`] — (API version 0 only, opt-in) verifies or
//!    inserts the `_g` guard prologue at every `loop`.
//! 5. [`validator::validate`] — the full SetHook-derived hard-error and
//!    warning rule set, run against the final bytes.
//!
//! `check` runs only stage 4 (+ the native guard checker), against
//! arbitrary input (including non-Rust/C-built hooks). `build`/`clean` run
//! every applicable stage in order.
//!
//! This crate relaxes `clippy::arithmetic_side_effects` relative to the
//! workspace default: unlike `hooks-core`/`hooks-lib` (which run *inside*
//! the wasm guest, where a panic is a validation-breaking bug), `hooks-build`
//! is an ordinary host-side CLI tool. Its arithmetic is almost entirely wasm
//! index-space bookkeeping (function/global counts, offsets) that is
//! structurally bounded by the 65,535-byte SetHook size limit — nowhere
//! near `u32::MAX` — so a panic here would mean a genuine bug, not a
//! reachable guest-facing failure mode. `clippy::unwrap_used`,
//! `clippy::expect_used`, `clippy::panic`, and `clippy::indexing_slicing`
//! remain denied, as inherited from the workspace.
#![allow(clippy::arithmetic_side_effects)]

use anyhow::Context as _;

mod cleaner;
mod fee;
mod flatten;
mod guard;
mod guard_native;
mod ir;
mod optimize;
mod unnest;
mod validator;
pub mod whitelist;

pub use cleaner::clean;
pub use fee::{FeeEstimate, estimate_fee};
pub use flatten::{FlattenReport, flatten};
pub use guard::auto_guard;
pub use guard_native::{GuardVerdict, NativeGuardError, validate_guards_native};
pub use optimize::{OptimizeReport, optimize};
pub use unnest::{UnnestReport, unnest};
pub use validator::{ValidationReport, validate};

/// The Hook API version a module targets. Determines whether the guard
/// pass/verifier runs at all (§6.3): version 1 ("Gas"-type hooks) has no
/// static instruction-count analysis and thus no guard requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApiVersion {
    /// HookApiVersion 0 ("Guard"-type hooks): loops must be guarded.
    #[default]
    V0,
    /// HookApiVersion 1 ("Gas"-type hooks): no guard requirement.
    V1,
}

/// Options threaded through every pipeline stage.
#[derive(Debug, Clone)]
pub struct Options {
    /// Which Hook API version this module targets.
    pub api_version: ApiVersion,
    /// If true, missing loop guards are inserted rather than treated as a
    /// hard error. Default off (see `docs/DESIGN.md` §10.1).
    pub auto_guard: bool,
    /// The `maxiter` value used for auto-inserted guards.
    pub default_maxiter: u32,
    /// If true, a module exceeding the 65,535-byte SetHook limit is still
    /// written to disk (clearly marked invalid) instead of erroring. Never
    /// honored by `check`.
    pub allow_oversize: bool,
    /// If true, run `wasm-opt -Oz` (see [`optimize::optimize`]) between the
    /// cleaner and the flatten pass. Off by default: it requires a system
    /// `wasm-opt` executable, and every later stage still re-validates its
    /// output in full, so turning it on can turn a previously-passing build
    /// into a failing one if `-Oz` produces a shape flatten/unnest/the guard
    /// checker rejects (`docs/DESIGN.md` §6).
    pub optimize: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            api_version: ApiVersion::default(),
            auto_guard: false,
            default_maxiter: 16,
            allow_oversize: false,
            optimize: false,
        }
    }
}

/// Runs the full `build`/`clean` pipeline: clean, then (if requested) the
/// `wasm-opt` pass, then (API version 0 only) flatten, then (API version 0
/// only) unnest, then (if requested and applicable) the guard pass, then
/// validate the final bytes. Returns the final bytes plus the validation
/// report. `build`/`clean` never emit bytes that fail hard-error validation
/// unless `opts.allow_oversize` downgrades the *size* rule specifically —
/// every other hard error still aborts.
///
/// Pipeline order for API version 0 (`docs/DESIGN.md` §6.2b/§6.2c/§6.3):
/// clean → optimize (opt-in, `-Oz`) → flatten (ensures `_g` is imported, R1,
/// and that the type section holds only import/entry types, R2) → unnest
/// (collapses the LLVM error ladder so nesting depth stays under the
/// vendored checker's 32-level limit) → auto-guard/guard-verify → Rust
/// validator → the vendored upstream guard checker, which is the final gate
/// — see [`verify`]. API version 1 skips flatten and unnest entirely (gas
/// hooks have no such restriction).
///
/// If `opts.optimize` is set, every stage after the cleaner still runs
/// exactly as it would against un-optimized input — `wasm-opt`'s output is
/// never trusted blindly. If any of those stages then fails, the resulting
/// error is annotated to suggest retrying without `--optimize`, since a
/// `-Oz` transform that flatten/unnest/the guard checker rejects is the most
/// likely explanation.
pub fn run_pipeline(wasm: &[u8], opts: &Options) -> anyhow::Result<(Vec<u8>, ValidationReport)> {
    let cleaned = cleaner::clean(wasm, opts)?;

    if !opts.optimize {
        let (guarded, report) = flatten_unnest_guard_verify(cleaned, opts)?;
        return Ok((guarded, report));
    }

    let before_bytes = cleaned.len();
    let before_guard = pre_optimize_guard_verdict(&cleaned, opts);
    let optimized = optimize::optimize(&cleaned)
        .context("wasm-opt (--optimize) failed to run; drop --optimize to build without it")?;

    match flatten_unnest_guard_verify(optimized, opts) {
        Ok((guarded, mut report)) => {
            report.optimize_report = Some(optimize::OptimizeReport {
                before_bytes,
                after_bytes: guarded.len(),
                before_fee: fee::estimate_fee(before_bytes),
                after_fee: fee::estimate_fee(guarded.len()),
                before_guard,
                after_guard: report.guard_verdict,
            });
            Ok((guarded, report))
        }
        Err(e) => Err(e.context(
            "this failure occurred on the wasm-opt (--optimize) output; retry without \
             --optimize to check whether wasm-opt is the cause",
        )),
    }
}

/// Runs flatten (API version 0 only) → unnest (API version 0 only) →
/// auto-guard/guard-verify (if requested and applicable) → [`verify`] over
/// an already-cleaned (and, optionally, already-`wasm-opt`'d) module. Shared
/// by both the plain and `--optimize` paths of [`run_pipeline`] so the two
/// never diverge in behavior beyond the extra `wasm-opt` step itself.
fn flatten_unnest_guard_verify(
    cleaned: Vec<u8>,
    opts: &Options,
) -> anyhow::Result<(Vec<u8>, ValidationReport)> {
    let flattened = if opts.api_version == ApiVersion::V0 {
        let (bytes, report) = flatten::flatten(&cleaned)?;
        for note in &report.notes {
            eprintln!("note: {note}");
        }
        bytes
    } else {
        cleaned
    };
    let unnested = if opts.api_version == ApiVersion::V0 {
        let (bytes, report) = unnest::unnest(&flattened)?;
        for note in &report.notes {
            eprintln!("note: {note}");
        }
        bytes
    } else {
        flattened
    };
    let guarded = if opts.auto_guard && opts.api_version == ApiVersion::V0 {
        guard::auto_guard(&unnested, opts)?
    } else {
        unnested
    };
    let report = verify(&guarded, opts)?;
    Ok((guarded, report))
}

/// Best-effort worst-case instruction counts for the cleaned-but-not-yet-
/// `wasm-opt`'d module, purely for the `--optimize` before/after comparison.
/// The vendored guard checker requires the flatten/unnest transforms first
/// (`docs/DESIGN.md` §6.2b/§6.2c), so this runs a throwaway copy of them;
/// any failure along the way (including a checker rejection) yields `None`
/// rather than aborting the build — this is informational only, never a
/// gate.
fn pre_optimize_guard_verdict(cleaned: &[u8], opts: &Options) -> Option<GuardVerdict> {
    if opts.api_version != ApiVersion::V0 {
        return None;
    }
    let (flattened, _) = flatten::flatten(cleaned).ok()?;
    let (unnested, _) = unnest::unnest(&flattened).ok()?;
    let unnested = if opts.auto_guard {
        guard::auto_guard(&unnested, opts).ok()?
    } else {
        unnested
    };
    guard_native::validate_guards_native(&unnested).ok()
}

/// Validates `wasm` and reconciles the Rust validator's verdict with the
/// vendored upstream guard checker's verdict (`docs/DESIGN.md` §6.5), which
/// is authoritative for API version 0.
///
/// - The [`MAX_SIZE`](validator::MAX_SIZE) byte-limit gate is a hooks-build/
///   SetHook policy the upstream checker has no opinion on at all (it isn't
///   something both sides evaluate and might disagree on), so it is not
///   subject to the "native wins" reconciliation below: an oversize module
///   (without `opts.allow_oversize`) is always rejected, exactly as the pure
///   Rust [`validate`] would.
/// - Otherwise, for API version 0: the Rust validator runs first (it has
///   better diagnostics — precise function/instruction-offset locations —
///   the upstream checker's log lacks), then the native checker runs as the
///   authoritative verdict:
///   - Native rejects ⇒ overall rejection, with the upstream log verbatim,
///     even if the Rust validator accepted the module.
///   - Native accepts ⇒ overall acceptance (with the worst-case instruction
///     counts attached via [`ValidationReport::guard_verdict`]), even if the
///     Rust validator flagged something — in that case the Rust findings
///     are downgraded to warnings and the divergence is noted explicitly,
///     never silently dropped.
/// - For API version 1, the native checker does not apply at all (it is the
///   guard-hook validator; version 1 "Gas"-type hooks have no guard
///   requirement) — this is exactly the pure Rust [`validate`].
pub fn verify(wasm: &[u8], opts: &Options) -> anyhow::Result<ValidationReport> {
    let size_hard_fail = wasm.len() > validator::MAX_SIZE && !opts.allow_oversize;
    let rust_result = validator::validate(wasm, opts);

    if size_hard_fail || opts.api_version != ApiVersion::V0 {
        return rust_result;
    }

    match guard_native::validate_guards_native(wasm) {
        Ok(verdict) => {
            let mut report = match rust_result {
                Ok(report) => report,
                Err(rust_err) => {
                    let mut report = ValidationReport::default();
                    report.warnings.push(format!(
                        "DIVERGENCE: the Rust validator flagged issue(s) that the authoritative \
                         upstream guard checker accepted; the native verdict wins and the \
                         module is treated as valid. Rust findings:\n{rust_err}"
                    ));
                    report
                }
            };
            report.guard_verdict = Some(verdict);
            Ok(report)
        }
        Err(native_err) => {
            let mut msg = String::new();
            match &rust_result {
                Ok(report) => {
                    for w in &report.warnings {
                        msg.push_str(&format!("rust validator warning: {w}\n"));
                    }
                    msg.push_str(
                        "DIVERGENCE: the Rust validator accepted this module, but the \
                         authoritative upstream guard checker rejected it. The native verdict \
                         wins.\n\n",
                    );
                }
                Err(rust_err) => {
                    msg.push_str("the Rust validator also flagged issues:\n");
                    msg.push_str(&rust_err.to_string());
                    msg.push_str("\n\n");
                }
            }
            msg.push_str(&format!(
                "upstream guard checker rejected the module (authoritative verdict):\n{native_err}"
            ));
            anyhow::bail!(msg)
        }
    }
}
