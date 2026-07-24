//! `hooks-lib` — ergonomic, Rust-idiomatic wrapper over `hooks-core`.
//!
//! This is the crate Hook developers import directly. It provides:
//! - [`error::HookError`] / [`error::Result`] — a typed error model over the
//!   raw negative-`i64` Hook API error codes.
//! - [`types`] — fixed-size buffer aliases for protocol-fixed shapes
//!   (`AccountId`, `Hash`, `Keylet`, ...).
//! - [`buf_eq`] — loop-free, panic-free equality checks for those fixed-size
//!   buffers (use instead of `==`, which can compile to an unguarded
//!   `compiler_builtins` `bcmp` loop).
//! - [`xfl::XFL`] — the Xahau decimal floating-point type.
//! - [`api`] — a `Result`-based wrapper for every Hook API function.
//! - [`pad!`], [`guard!`], [`guard_m!`], [`accept!`], [`rollback!`], `trace!` family —
//!   terse macros for common patterns (see `macros.rs`).
//! - [`hook_errors!`] / [`exit_on_err!`] — define a `#[repr(i64)]` user error
//!   enum and convert `Result<T, YourEnum>` into a `rollback!` at the hook's
//!   boundary (see `errors.rs`).
//! - An optional panic handler (feature `panic-handler`, default-on) that
//!   rolls the hook back instead of leaving an unhandled panic.
//!
//! `#![no_std]`: this crate targets `wasm32v1-none` Hook binaries as well as
//! host builds (for tests/doctests, which run against `hooks-core`'s
//! deterministic `NOT_IMPLEMENTED` stubs).
//!
//! `hooks-core` is re-exported as [`raw`] for direct access to raw Hook API
//! declarations and every C-verbatim constant (`sfcodes`, `tts`, `ls_flags`,
//! `tx_flags`, `consts`) — this is the path `guard!`/`guard_m!` expand
//! through (`$crate::raw::_g`), and it is also how a hook can drop to the
//! raw FFI layer for anything not yet covered by [`api`]. A plain `pub use
//! hooks_core as raw;` (rather than `pub mod raw { pub use hooks_core::*; }`)
//! keeps that path a single, direct alias with no extra module indirection.

#![no_std]
// Nightly only: lets `txn_template!` synthesize `set_<field>` setter names
// (`${concat(set_, $field)}`) instead of naming setters after the bare field.
#![feature(macro_metavar_expr_concat)]

pub mod api;
pub mod buf_eq;
pub mod error;
mod errors;
mod macros;
pub mod static_cell;
pub mod txn;
pub mod types;
pub mod xfl;

// `pad!` expands to `$crate::padded_bytes(...)`; the helper lives in the
// private `macros` module, so re-export it (hidden) at the crate root.
#[doc(hidden)]
pub use macros::padded_bytes;

/// Direct re-export of `hooks-core`: raw Hook API declarations and every
/// C-verbatim constant. See the crate doc comment for why this is a plain
/// alias rather than a re-exporting wrapper module.
pub use hooks_core as raw;

/// Common imports for hook developers: `use hooks_lib::prelude::*;` pulls in
/// every `api::*` wrapper function, the fixed-size buffer type aliases, the
/// [`xfl::XFL`] type, [`error::HookError`]/[`error::Result`], and the
/// C-verbatim constant families (`sfXxx`, `ttXxx`, `lsfXxx`, `tfXxx`, and
/// `hookapi.h`'s `KEYLET_*`/`COMPARE_*`/... constants). Deliberately does
/// NOT re-export all of `hooks_core` (its raw `api::*` functions share names
/// with this crate's own wrappers, e.g. both define `state`) — only the
/// constant-only modules are pulled in, so there is no ambiguity between a
/// prelude-imported name and a hooks-lib wrapper.
pub mod prelude {
    pub use crate::api::*;
    pub use crate::buf_eq::*;
    pub use crate::error::{HookError, Result};
    pub use crate::static_cell::HookStatic;
    pub use crate::types::*;
    pub use crate::xfl::XFL;
    pub use hooks_core::{consts::*, ls_flags::*, sfcodes::*, tts::*, tx_flags::*};
}

/// Distinctive negative code used by the panic handler below when rolling
/// back. Chosen well outside the documented Hook API error-code range
/// (`-1..=-45`, plus the one irregular `-10024` for `INVALID_FLOAT`) so it
/// can never be confused with a real Hook API error.
#[cfg(all(target_arch = "wasm32", feature = "panic-handler"))]
const PANIC_ROLLBACK_CODE: i64 = -999_999;

/// Panic handler for wasm Hook binaries: rolls the hook back with a fixed
/// message instead of leaving an unhandled panic (which has no defined
/// behavior on the Hook host and, per DESIGN.md §2 C7, panic machinery is
/// something hooks-lib is built to avoid needing in the first place — this
/// handler is the last-resort backstop, not the primary correctness
/// mechanism). Enabled by the default-on `panic-handler` feature; disable it
/// if a hook wants to supply its own.
#[cfg(all(target_arch = "wasm32", feature = "panic-handler"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        let _ = hooks_core::rollback(b"panic".as_ptr() as u32, 5, PANIC_ROLLBACK_CODE);
    }
    core::arch::wasm32::unreachable()
}

/// Host-target panic handler for `no_std` hook crates, behind the
/// **non-default** `host-panic-handler` feature.
///
/// A hook crate is a `no_std` cdylib, so even a host `cargo check` (what
/// rust-analyzer runs for completion and diagnostics) demands a
/// `#[panic_handler]` — but the wasm handler above is target-gated, and
/// hooks-lib cannot provide one unconditionally on the host: any `std`
/// consumer (like hooks-lib's own test harness) would then hit a duplicate
/// lang item. Hook crates opt in via
/// `hooks-lib = { ..., features = ["host-panic-handler"] }`, which makes
/// host analysis work; the handler itself is never reached (host builds of
/// hook crates are for analysis only, not execution).
#[cfg(all(not(target_arch = "wasm32"), feature = "host-panic-handler"))]
#[panic_handler]
#[allow(clippy::empty_loop)] // analysis-only target; never executed
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
