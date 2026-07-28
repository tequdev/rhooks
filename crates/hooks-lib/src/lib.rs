//! `hooks-lib` — ergonomic, Rust-idiomatic wrapper over `hooks-core`.
//!
//! This is the crate Hook developers import directly. It provides:
//! - [`error::HookError`] / [`error::Result`] — a typed error model over the
//!   raw negative-`i64` Hook API error codes.
//! - [`types`] — fixed-size, `#[repr(transparent)]` newtypes for
//!   protocol-fixed shapes (`AccountId`, `Hash`, `Keylet`, ...).
//! - [`convert::ToBytes`]/[`convert::FromBytes`] — boundary conversion
//!   traits for encoding/decoding fixed-size values to/from byte buffers.
//! - [`state`] — a typed layer over hook state (`state_get`,
//!   `state_set_typed`, `state_update_typed`, and the [`state_keys!`] macro
//!   for declaring a state-key enum) built on top of [`convert`].
//! - [`buf_eq`] — loop-free, panic-free equality checks for those fixed-size
//!   buffers/newtypes (use instead of `==`, which can compile to an
//!   unguarded `compiler_builtins` `bcmp` loop).
//! - [`xfl::XFL`] — the Xahau decimal floating-point type, with
//!   `Result`-returning `Add`/`Sub`/`Mul`/`Div`/`Neg` operators plus
//!   `PartialEq`/`PartialOrd`.
//! - [`xfl_unchecked::XFLUnchecked`] — a poison-propagating hot-path
//!   counterpart to `XFL`: unchecked operators, one `validate()` call at
//!   the end of a chain.
//! - [`tx_type::TxType`] — a typed mirror of the raw `tt*` transaction-type
//!   codes (`hooks_core::tts`), decoded from [`api::otxn::otxn_type`]'s
//!   raw `u16`.
//! - [`api`] — a `Result`-based wrapper for every Hook API function.
//! - [`pad!`], [`guard!`], [`guard_m!`], [`accept!`], [`rollback!`], `trace!` family —
//!   terse macros for common patterns (see `macros.rs`).
//! - [`hook`] / [`cbak`] — attribute macros that turn a plain, argument-less
//!   `fn name() -> i64` into the wasm export shape the Hook host requires
//!   (see `hooks-macros`'s crate doc comment).
//! - [`account_id!`](account_id) — decodes a classic Ripple/Xahau r-address
//!   into an [`types::AccountId`] literal entirely at compile time (zero
//!   runtime/wasm-size cost).
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

pub mod api;
pub mod buf_eq;
pub mod convert;
pub mod error;
mod errors;
mod macros;
pub mod state;
pub mod static_cell;
pub mod tx_type;
pub mod txn;
pub mod types;
pub mod xfl;
pub mod xfl_unchecked;

// `pad!` expands to `$crate::padded_bytes(...)`; the helper lives in the
// private `macros` module, so re-export it (hidden) at the crate root.
#[doc(hidden)]
pub use macros::padded_bytes;

/// Direct re-export of `hooks-core`: raw Hook API declarations and every
/// C-verbatim constant. See the crate doc comment for why this is a plain
/// alias rather than a re-exporting wrapper module.
pub use hooks_core as raw;

/// Turns a plain, argument-less `fn name() -> i64` into the Hook host's
/// required `hook` export (`#[unsafe(no_mangle)] pub extern "C" fn hook(
/// _reserved: u32) -> i64`). See `hooks_macros::hook`'s doc comment for the
/// exact requirements and generated shape.
pub use hooks_macros::hook;

/// Like [`hook`], but exports `cbak` instead — for the optional callback a
/// Hook module can export, invoked when a transaction it previously emitted
/// settles.
pub use hooks_macros::cbak;

/// Decodes a classic XRPL/Xahau r-address (base58check, e.g.
/// `"rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh"`) into an [`types::AccountId`]
/// literal — **entirely at compile time**, inside the proc-macro (host-side
/// base58 decode + double-SHA256 checksum verification, hand-written in
/// `hooks-macros` — see its crate doc comment for why not a `sha2`/`bs58`
/// dependency). The expansion is a plain `AccountId([u8; 20])` literal, so
/// this costs the compiled Hook wasm binary **nothing**: no decode logic
/// ships in the binary at all, and it works in `const`/`static` position.
///
/// This replaces hand-computing/hardcoding a 20-byte array for a
/// well-known address — e.g. `examples/80_reward`/`examples/81_govern`
/// both hand-hardcode `GENESIS_ACCOUNT`, the Xahau/XRPL standalone-network
/// genesis/master account, as a raw `AccountId([0xB5, 0xF7, ...])` literal;
/// `account_id!("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh")` produces the exact
/// same value from the address string directly.
///
/// A malformed address (bad character, wrong length, wrong version byte,
/// or a failed checksum) is a `compile_error!` at the macro call site
/// naming exactly what was wrong — never a runtime failure.
///
/// # Examples
///
/// ```
/// use hooks_lib::account_id;
/// use hooks_lib::types::AccountId;
///
/// // The Xahau/XRPL standalone-network genesis/master account (seed
/// // "masterpassphrase") — see the note above about `GENESIS_ACCOUNT`.
/// const OWNER: AccountId = account_id!("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh");
/// assert_eq!(
///     OWNER.0,
///     [
///         0xB5, 0xF7, 0x62, 0x79, 0x8A, 0x53, 0xD5, 0x43, 0xA0, 0x14, 0xCA, 0xF8, 0xB2, 0x97,
///         0xCF, 0xF8, 0xF2, 0xF9, 0x37, 0xE8
///     ]
/// );
///
/// // `ACCOUNT_ZERO`, XRPL's well-known all-zero special address — also
/// // usable in a `static`, not just a bare `const`, exactly like
/// // `AccountId::zeroed()` elsewhere in this crate.
/// static ACCOUNT_ZERO: AccountId = account_id!("rrrrrrrrrrrrrrrrrrrrrhoLvTp");
/// assert_eq!(ACCOUNT_ZERO.0, [0u8; 20]);
/// ```
///
/// A checksum mismatch (last character `h` -> `H`) fails to compile:
/// ```compile_fail
/// hooks_lib::account_id!("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTH");
/// ```
///
/// A wrong version byte (this address doesn't even start with `'r'`) fails
/// to compile:
/// ```compile_fail
/// hooks_lib::account_id!("sJHw2iRxXngPFKZvYbjkfifqt8CJghksMM");
/// ```
///
/// A truncated address (wrong decoded length) fails to compile:
/// ```compile_fail
/// hooks_lib::account_id!("rHb9CJAWyB4rj91VRWn96DkukG4bwdty");
/// ```
///
/// An invalid character (`'0'` is not in the Ripple base58 alphabet) fails
/// to compile:
/// ```compile_fail
/// hooks_lib::account_id!("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyT0");
/// ```
pub use hooks_macros::account_id;

// `txn_template!` expands `[<set_ $field>]` splice markers through
// `$crate::__paste!`, its own stable replacement for nightly's
// `${concat(...)}` (see `txn.rs`); re-export it (hidden) at the crate root
// so `$crate::__paste!` resolves regardless of which crate invokes
// `txn_template!`.
#[doc(hidden)]
pub use hooks_macros::paste as __paste;

/// Common imports for hook developers: `use hooks_lib::prelude::*;` pulls in
/// every `api::*` wrapper function, the fixed-size buffer type aliases, the
/// [`xfl::XFL`]/[`xfl_unchecked::XFLUnchecked`]/[`tx_type::TxType`] types,
/// [`error::HookError`]/[`error::Result`], and the C-verbatim constant
/// families (`sfXxx`, `ttXxx`, `lsfXxx`, `tfXxx`, and `hookapi.h`'s
/// `KEYLET_*`/`COMPARE_*`/... constants). Deliberately does NOT re-export
/// all of `hooks_core` (its raw `api::*` functions share names with this
/// crate's own wrappers, e.g. both define `state`) — only the constant-only
/// modules are pulled in, so there is no ambiguity between a
/// prelude-imported name and a hooks-lib wrapper.
pub mod prelude {
    pub use crate::api::*;
    pub use crate::buf_eq::*;
    pub use crate::convert::{FixedRead, FromBytes, ToBytes};
    pub use crate::error::{HookError, Result};
    pub use crate::state::{
        StateKeyEncode, state_foreign_get, state_foreign_set_typed, state_foreign_update_typed,
        state_get, state_set_typed, state_update_typed,
    };
    pub use crate::static_cell::HookStatic;
    pub use crate::tx_type::TxType;
    pub use crate::types::*;
    pub use crate::xfl::XFL;
    pub use crate::xfl_unchecked::XFLUnchecked;
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
