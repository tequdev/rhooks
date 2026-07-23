//! `hooks-build` — a CLI and library that turns a Rust crate targeting
//! `wasm32v1-none` into a SetHook-valid WASM binary.
//!
//! The pipeline (see `docs/DESIGN.md` §6) is three independently-testable,
//! byte-in/byte-out stages:
//!
//! 1. [`cleaner::clean`] — drops custom sections, restricts exports to
//!    `hook`/`cbak`, garbage-collects unreachable functions/globals, and
//!    re-encodes the module with a whole-module index remap.
//! 2. [`guard::auto_guard`] — (API version 0 only, opt-in) verifies or
//!    inserts the `_g` guard prologue at every `loop`.
//! 3. [`validator::validate`] — the full SetHook-derived hard-error and
//!    warning rule set, run against the final bytes.
//!
//! `check` runs only stage 3, against arbitrary input (including
//! non-Rust/C-built hooks). `build`/`clean` run stages 1–3 in order.
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

mod cleaner;
mod fee;
mod guard;
mod ir;
mod validator;
pub mod whitelist;

pub use cleaner::clean;
pub use fee::{FeeEstimate, estimate_fee};
pub use guard::auto_guard;
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
}

impl Default for Options {
    fn default() -> Self {
        Self {
            api_version: ApiVersion::default(),
            auto_guard: false,
            default_maxiter: 16,
            allow_oversize: false,
        }
    }
}

/// Runs the full `build`/`clean` pipeline: clean, then (if requested and
/// applicable) the guard pass, then validate the final bytes. Returns the
/// final bytes plus the validation report. `build`/`clean` never emit bytes
/// that fail hard-error validation unless `opts.allow_oversize` downgrades
/// the *size* rule specifically — every other hard error still aborts.
pub fn run_pipeline(wasm: &[u8], opts: &Options) -> anyhow::Result<(Vec<u8>, ValidationReport)> {
    let cleaned = cleaner::clean(wasm, opts)?;
    let guarded = if opts.auto_guard && opts.api_version == ApiVersion::V0 {
        guard::auto_guard(&cleaned, opts)?
    } else {
        cleaned
    };
    let report = validator::validate(&guarded, opts)?;
    Ok((guarded, report))
}
