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
//!   for declaring a state-key enum) built on top of [`convert`]. Pair a key
//!   type with its one value type via [`state::TypedStateKey`]/
//!   [`state_key_value!`] and use `state_get_kv`/`state_set_kv`/
//!   `state_update_kv` for a key/value mismatch that's a compile
//!   error instead of a latent bug (see [`state::TypedStateKey`]'s doc
//!   comment).
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
//! - [`hook_errors!`] / [`exit_on_err!`] — define a `#[repr(i64)]` user error
//!   enum and convert `Result<T, YourEnum>` into a `rollback!` at the hook's
//!   boundary (see `errors.rs`).
//! - [`HookData`] — derive macro turning a fixed-size, named-field struct
//!   into a zero-cost [`convert::ToBytes`]/[`convert::FromBytes`]/
//!   [`convert::FixedRead`] triple, for composite (multi-field) state keys,
//!   state values, and `otxn_param`/`hook_param` payloads (see its own doc
//!   comment for the full writeup).
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

/// Derives [`convert::ToBytes`]/[`convert::FromBytes`]/[`convert::FixedRead`]
/// for a fixed-size, named-field struct — so the struct can be used
/// directly as a composite (multi-field) hook-state value, an
/// `otxn_param`/`hook_param` payload, or — via [`state::StateKeyEncode`]'s
/// blanket impl over any [`convert::ToBytes`] type (see [`state`]'s module
/// doc comment) — a composite state *key*, with no hand-packed byte buffer
/// anywhere.
///
/// # Grammar
///
/// ```text
/// #[derive(HookData)]
/// $vis struct Name {
///     $vis field: FieldType,
///     ...
/// }
/// ```
///
/// - A plain (non-generic) struct with **named fields only** — no tuple
///   structs, no unit structs, no enums, no unions.
/// - At least one field.
/// - Every field's type must implement [`convert::ToBytes`] +
///   [`convert::FromBytes`]: any of this crate's fixed-size primitives
///   (`u8`/`u16`/`u32`/`u64`/`i64`), [`xfl::XFL`], any `hooks_lib::types`
///   newtype (`AccountId`, `Hash`, ...), a raw `[u8; N]`, or another
///   `#[derive(HookData)]` struct (nesting composes for free — see below).
///   A field of any other (variable-length) type fails to compile with an
///   ordinary rustc trait-bound error naming the missing `ToBytes`/
///   `FromBytes` impl against the generated code — this derive does not
///   implement its own type checker.
///
/// # What gets generated
///
/// - `impl ToBytes for Name` / `impl FromBytes for Name` / `impl FixedRead
///   for Name`: fields are encoded **back-to-back, in declaration order**,
///   each contributing exactly its own `ToBytes::MAX_LEN` bytes — no
///   padding, no per-field length prefix, no reordering.
/// - `Name::LEN: usize` — the total encoded length (`Name::MAX_LEN` under
///   another name, as an inherent const so call sites don't need `use
///   hooks_lib::convert::ToBytes;` just to name it), with a generated
///   rustdoc table listing the field layout.
///
/// # Zero-cost by construction
///
/// Every field offset is a compile-time constant (a chain of `const`
/// declarations built from each field's own `ToBytes::MAX_LEN`, resolved at
/// compile time), and every field read/write delegates straight to that
/// field's own `ToBytes::write`/`FromBytes::read` — the identical "fixed,
/// unrolled offsets, no runtime-computed length" shape this crate already
/// hand-writes for [`txn_template!`]'s generated setters. There is no
/// per-field loop, and (for a total size the toolchain still lowers to
/// inlined stores rather than a `memset`/`memcpy` builtin call — empirically
/// up to 32 bytes at this crate's release profile, see
/// [`state`]'s `MAX_TYPED_STATE_LEN` doc comment) no unguarded loop at all.
/// `examples/12_typed-data`'s README measures this directly: a
/// `#[derive(HookData)]` struct and a hand-packed equivalent compile to the
/// same worst-case instruction count.
///
/// # Nesting
///
/// A `#[derive(HookData)]` struct can itself be a field of another — since
/// the derive only ever requires a field's type to implement `ToBytes`/
/// `FromBytes`/`FixedRead`, and every derived struct does, nesting needs no
/// special support:
///
/// ```
/// use hooks_lib::HookData;
/// use hooks_lib::prelude::*;
///
/// #[derive(HookData)]
/// struct Inner {
///     count: u32,
/// }
///
/// #[derive(HookData)]
/// struct Outer {
///     tag: u8,
///     inner: Inner,
/// }
///
/// assert_eq!(Outer::LEN, 1 + 4);
/// ```
///
/// # Examples
///
/// A composite state key (a tag byte plus an `AccountId`) and a composite
/// state value (an amount, a deadline, and a flags byte), paired via
/// [`state_key_value!`] and used with [`state::state_get_kv`]/
/// [`state::state_set_kv`] — no `state_keys!` declaration, no
/// hand-packed byte buffer, and (unlike the loose [`state::state_get`]/
/// [`state::state_set_typed`], which take the value type as an independent
/// generic parameter — see [`state::TypedStateKey`]'s doc comment) no way to
/// accidentally read/write `DepositKey`'s entry as some other struct's value
/// type:
///
/// ```
/// use hooks_lib::HookData;
/// use hooks_lib::prelude::*;
/// use hooks_lib::state_key_value;
///
/// #[derive(HookData, Clone, Copy)]
/// struct DepositKey {
///     tag: u8,
///     owner: AccountId,
/// }
///
/// #[derive(HookData, Clone, Copy, Debug, PartialEq)]
/// struct DepositValue {
///     amount: u64,
///     deadline: u32,
///     flags: u8,
/// }
///
/// state_key_value!(DepositKey => DepositValue);
///
/// assert_eq!(DepositKey::LEN, 1 + 20);
/// assert_eq!(DepositValue::LEN, 8 + 4 + 1);
///
/// let key = DepositKey {
///     tag: 1,
///     owner: AccountId::default(),
/// };
///
/// // `NotImplemented` here is the host stub every Hook API call returns on
/// // a host build (see `hooks-core`) — this only proves the generated
/// // `TypedStateKey`/`state_get_kv` call chain compiles and runs,
/// // exactly like `state_keys!`'s own doctest.
/// assert_eq!(state_get_kv(&key), Err(HookError::NotImplemented));
/// ```
///
/// A struct used as a fixed-size `otxn_param`/`hook_param` payload, named
/// via [`param_name!`](crate::param_name) and read with
/// [`api::otxn::otxn_param_typed`] — again, unlike [`api::otxn::otxn_param_exact`]
/// (which takes the parameter name as an independent argument — see
/// [`convert::ParamName`]'s doc comment), there is no separate `name`
/// argument that could name a *different* parameter than the one `Config`
/// was declared for:
///
/// ```
/// use hooks_lib::HookData;
/// use hooks_lib::prelude::*;
/// use hooks_lib::param_name;
///
/// #[derive(HookData)]
/// struct Config {
///     min_amount: u64,
///     max_amount: u64,
/// }
///
/// param_name!(Config, b"CFG");
///
/// let cfg: Result<Config> = otxn_param_typed();
/// assert_eq!(cfg.err(), Some(HookError::NotImplemented));
/// ```
///
/// An enum is rejected at compile time (`HookData` only derives for a named-
/// field struct):
///
/// ```compile_fail
/// use hooks_lib::HookData;
///
/// #[derive(HookData)]
/// enum NotAStruct {
///     A,
///     B,
/// }
/// ```
///
/// A tuple struct is rejected the same way:
///
/// ```compile_fail
/// use hooks_lib::HookData;
///
/// #[derive(HookData)]
/// struct NotNamedFields(u32, u64);
/// ```
///
/// A field of a variable-length type (here, a bare slice reference) fails
/// to compile — not with a diagnostic this derive produces itself, but with
/// rustc's own trait-bound error against the generated `ToBytes`/`FromBytes`
/// impls, naming the missing trait:
///
/// ```compile_fail
/// use hooks_lib::HookData;
///
/// #[derive(HookData)]
/// struct VariableLength<'a> {
///     data: &'a [u8],
/// }
/// ```
///
/// A struct whose total encoded length exceeds the 32-byte state-key space
/// still derives fine (it works as a state *value*, or an `otxn_param`/
/// `hook_param` payload) — only *using it as a key* (via
/// [`state::StateKeyEncode`]'s blanket impl) is a compile error, and only at
/// the call site that actually tries:
///
/// ```compile_fail
/// use hooks_lib::HookData;
/// use hooks_lib::prelude::*;
///
/// #[derive(HookData)]
/// struct TooBigForAKey {
///     a: [u8; 20],
///     b: [u8; 20],
/// }
///
/// // `TooBigForAKey::LEN` is 40, over the 32-byte state key space.
/// let _ = state_get::<u64>(&TooBigForAKey {
///     a: [0; 20],
///     b: [0; 20],
/// });
/// ```
///
/// The loose [`state::state_get`]/[`state::state_set_typed`] take a key and
/// a value type as independent generic parameters, so nothing there stops
/// pairing a key with the *wrong* value type — [`state_key_value!`] plus
/// [`state::state_set_kv`] closes that: `value`'s type is checked
/// against the key's own declared [`state::TypedStateKey::Value`], so
/// passing a value meant for a different key is a compile error (see
/// [`state::TypedStateKey`]'s doc comment for the full rationale):
///
/// ```compile_fail
/// use hooks_lib::HookData;
/// use hooks_lib::prelude::*;
/// use hooks_lib::state_key_value;
///
/// #[derive(HookData, Clone, Copy)]
/// struct KeyA {
///     tag: u8,
/// }
///
/// #[derive(HookData, Clone, Copy)]
/// struct ValueA {
///     count: u32,
/// }
///
/// #[derive(HookData, Clone, Copy)]
/// struct ValueB {
///     amount: u64,
/// }
///
/// state_key_value!(KeyA => ValueA);
///
/// // ERROR: `ValueB` is not `KeyA`'s declared `Value` (`ValueA`).
/// let _ = state_set_kv(&KeyA { tag: 0 }, &ValueB { amount: 0 });
/// ```
pub use hooks_macros::HookData;

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
    pub use crate::convert::{FixedRead, FromBytes, ParamName, ToBytes};
    pub use crate::error::{HookError, Result};
    pub use crate::state::{
        StateKeyEncode, TypedStateKey, state_foreign_get, state_foreign_get_kv,
        state_foreign_set_kv, state_foreign_set_typed, state_foreign_update_kv,
        state_foreign_update_typed, state_get, state_get_kv, state_set_kv, state_set_typed,
        state_update_kv, state_update_typed,
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
