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
//!   `state_set_loose`, `state_update_loose`, and the [`state_keys!`] macro
//!   for declaring a state-key enum) built on top of [`convert`]. Pair a key
//!   type with its one value type via [`state::TypedStateKey`]/
//!   [`hook_state!`] and use `state_get_typed`/`state_set_typed`/
//!   `state_update_typed` for a key/value mismatch that's a compile
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
//! - Four narrow derive macros, all built on the same fixed-offset
//!   [`convert::ToBytes`]/[`convert::FromBytes`] codegen, one per role a
//!   composite (multi-field) struct plays in this crate — deliberately four
//!   separate derives rather than one covering everything, so each generates
//!   only what its role actually needs (see [`HookKey`]'s doc comment for the
//!   full rationale):
//!   - [`HookKey`] — a composite **hook-state key** (write-only, plus an
//!     explicit [`state::StateKeyEncode`] impl with the 32-byte key-space
//!     bound checked at derive time).
//!   - [`HookData`] — a composite **hook-state value** (the full
//!     `ToBytes`/`FromBytes`/`FixedRead` triple, read back and decoded by
//!     `state_get`/`state_get_typed`).
//!   - [`ParamName`] — a composite **Hook API parameter name** (write-only,
//!     with the Hook API's own 1–32-byte parameter-name bound checked at
//!     derive time).
//!   - [`ParamValue`] — a **Hook API parameter value** (read-only:
//!     `FromBytes`/`FixedRead`, no `ToBytes` — a parameter value is decoded,
//!     never itself used to locate anything).
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

/// Derives [`convert::ToBytes`] and an explicit [`state::StateKeyEncode`]
/// impl for a fixed-size, named-field struct used as a **composite
/// hook-state key** — a tag byte plus an `AccountId`, say — with no
/// hand-packed byte buffer anywhere. See [`HookData`] for the state-*value*
/// counterpart, [`ParamName`] for the analogous Hook API parameter-*name*
/// role, and [`ParamValue`] for the analogous parameter-*value* role.
///
/// # Why a separate derive from [`HookData`]
///
/// A hook-state *key* and a hook-state *value* share the same "fixed-offset,
/// named-field struct" shape, but play genuinely different roles, so this
/// crate keeps them as two narrower derives instead of one derive (and one
/// blanket impl) covering both:
///
/// - A key is only ever **encoded outward** — handed to `state`/
///   `state_foreign` to *locate* an entry — never read back and decoded as
///   itself (the *value* stored at that key is what gets read back).
///   `HookKey` reflects that by generating only [`convert::ToBytes`] plus
///   [`state::StateKeyEncode`]: no `FromBytes`, no `FixedRead`, no inherent
///   `LEN` const.
/// - A key has a bound the Hook API's fixed key space imposes — a key is
///   always exactly 32 bytes, zero-padded — distinct from a value's lack of
///   any size cap (beyond this crate's own `MAX_TYPED_STATE_LEN`
///   convenience limit — see [`state`]'s module doc comment). `HookKey`
///   checks the 32-byte bound **at derive time**, unconditionally: a
///   `#[derive(HookKey)]` struct that encodes to 33+ bytes fails to compile
///   at its own definition, before it's ever used as a key at all.
/// - Only a `#[derive(HookKey)]` struct, a [`state_keys!`](crate::state_keys)
///   enum, or [`types::StateKey`] itself implements
///   [`state::StateKeyEncode`] — an ordinary `#[derive(HookData)]` *value*
///   struct does **not** automatically qualify as a key, so a state value
///   can never be passed where a key is expected by accident.
///
/// # Grammar
///
/// Identical field grammar to [`HookData`] (see its doc comment): a plain,
/// non-generic, named-field struct with at least one field, every field a
/// fixed-size type implementing [`convert::ToBytes`] (nesting another
/// `#[derive(HookKey)]` or `#[derive(HookData)]` struct as a field works the
/// same way).
///
/// # What gets generated
///
/// - `impl ToBytes for Name`: fields encoded back-to-back, in declaration
///   order — identical codegen to [`HookData`]'s `ToBytes` impl (see its doc
///   comment's "What gets generated"/"Zero-cost by construction" sections);
///   this derive only adds the `StateKeyEncode` impl on top.
/// - `impl state::StateKeyEncode for Name`: encodes `self` via the `ToBytes`
///   impl above into a zero-padded 32-byte [`types::StateKey`], with a
///   compile-time (monomorphized) assert that `Name`'s encoded length fits.
///
/// # Examples
///
/// A composite state key (a tag byte plus an `AccountId`) paired with a
/// composite state value via [`hook_state!`] and used with
/// [`state::state_get_typed`]/[`state::state_set_typed`] — no `state_keys!`
/// declaration, no hand-packed byte buffer, and (unlike the loose
/// [`state::state_get`]/[`state::state_set_loose`], which take the value
/// type as an independent generic parameter — see
/// [`state::TypedStateKey`]'s doc comment) no way to accidentally read/write
/// `DepositKey`'s entry as some other struct's value type:
///
/// ```
/// use hooks_lib::{HookData, HookKey};
/// use hooks_lib::prelude::*;
/// use hooks_lib::hook_state;
///
/// #[derive(HookKey, Clone, Copy)]
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
/// hook_state!(DepositKey => DepositValue);
///
/// assert_eq!(DepositValue::LEN, 8 + 4 + 1);
///
/// let key = DepositKey {
///     tag: 1,
///     owner: AccountId::default(),
/// };
///
/// // `NotImplemented` here is the host stub every Hook API call returns on
/// // a host build (see `hooks-core`) — this only proves the generated
/// // `TypedStateKey`/`state_get_typed` call chain compiles and runs,
/// // exactly like `state_keys!`'s own doctest.
/// assert_eq!(state_get_typed(&key), Err(HookError::NotImplemented));
/// ```
///
/// An enum is rejected at compile time:
///
/// ```compile_fail
/// use hooks_lib::HookKey;
///
/// #[derive(HookKey)]
/// enum NotAStruct {
///     A,
///     B,
/// }
/// ```
///
/// A struct whose total encoded length exceeds the 32-byte state-key space
/// is rejected **at its own definition** — unlike [`HookData`], which has no
/// such bound at all (a state *value* has no fixed size cap):
///
/// ```compile_fail
/// use hooks_lib::HookKey;
///
/// #[derive(HookKey)]
/// struct TooBigForAKey {
///     a: [u8; 20],
///     b: [u8; 20],
/// }
/// ```
///
/// The loose [`state::state_get`]/[`state::state_set_loose`] take a key and
/// a value type as independent generic parameters, so nothing there stops
/// pairing a key with the *wrong* value type — [`hook_state!`] plus
/// [`state::state_set_typed`] closes that: `value`'s type is checked
/// against the key's own declared [`state::TypedStateKey::Value`], so
/// passing a value meant for a different key is a compile error (see
/// [`state::TypedStateKey`]'s doc comment for the full rationale):
///
/// ```compile_fail
/// use hooks_lib::{HookData, HookKey};
/// use hooks_lib::prelude::*;
/// use hooks_lib::hook_state;
///
/// #[derive(HookKey, Clone, Copy)]
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
/// hook_state!(KeyA => ValueA);
///
/// // ERROR: `ValueB` is not `KeyA`'s declared `Value` (`ValueA`).
/// let _ = state_set_typed(&KeyA { tag: 0 }, &ValueB { amount: 0 });
/// ```
pub use hooks_macros::HookKey;

/// Derives [`convert::ToBytes`]/[`convert::FromBytes`]/[`convert::FixedRead`]
/// for a fixed-size, named-field struct used as a **composite hook-state
/// value** — read back and decoded by `state_get`/`state_get_typed`, written by
/// `state_set_loose`/`state_set_typed`. See [`HookKey`] for the state-*key*
/// counterpart (and why it's a separate, narrower derive rather than
/// `HookData` also serving as a key), [`ParamName`] for the analogous Hook
/// API parameter-*name* role, and [`ParamValue`] for the analogous
/// parameter-*value* role (a `#[derive(HookData)]` struct also happens to
/// satisfy `ParamValue`'s `FromBytes`/`FixedRead` requirement and so *can*
/// be used as a parameter value directly — [`ParamValue`] is the narrower,
/// intent-revealing choice for a struct that is only ever a parameter
/// payload and never a state value).
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
/// See [`HookKey`]'s doc comment for a full key+value worked example
/// (`DepositKey`/`DepositValue`, paired via [`hook_state!`]). A `HookData`
/// struct also works directly as a state value with the loose
/// [`state::state_get`]/[`state::state_set_loose`] (no key pairing, `T`
/// named independently at the call site):
///
/// ```
/// use hooks_lib::HookData;
/// use hooks_lib::prelude::*;
///
/// #[derive(HookData, Clone, Copy, Debug, PartialEq)]
/// struct DepositValue {
///     amount: u64,
///     deadline: u32,
///     flags: u8,
/// }
///
/// assert_eq!(DepositValue::LEN, 8 + 4 + 1);
///
/// let key = StateKey::from([0u8; 32]);
/// let value: Result<Option<DepositValue>> = state_get(&key);
/// assert_eq!(value, Err(HookError::NotImplemented));
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
/// A `HookData` struct does **not** automatically work as a state *key* —
/// unlike the pre-4-derive design, there is no blanket
/// [`state::StateKeyEncode`] impl over every `ToBytes` type, so this fails
/// to compile (use [`HookKey`] instead):
///
/// ```compile_fail
/// use hooks_lib::HookData;
/// use hooks_lib::prelude::*;
///
/// #[derive(HookData)]
/// struct NotAKey {
///     a: [u8; 20],
/// }
///
/// // ERROR: `NotAKey` has no `StateKeyEncode` impl (`HookData` never
/// // generates one — use `HookKey` for a state key).
/// let _ = state_get::<u64>(&NotAKey { a: [0; 20] });
/// ```
pub use hooks_macros::HookData;

/// Derives [`convert::ToBytes`] (only — no [`convert::FromBytes`]/
/// [`convert::FixedRead`]) for a fixed-size, named-field struct used as a
/// **composite Hook API parameter name** — a name type implementing
/// [`convert::TypedParamName`] (see
/// [`hook_parameter!`](crate::hook_parameter)/
/// [`otxn_parameter!`](crate::otxn_parameter)'s composite form, `$Name =>
/// $Ty`, and [`crate::api::hook_ctx::hook_param_typed`]/
/// [`crate::api::otxn::otxn_param_typed`], which take **a reference to a name
/// value**). See [`HookKey`] for the analogous hook-state *key* role, and
/// [`ParamValue`] for the parameter *value* counterpart (what `$Ty` above
/// must satisfy).
///
/// # Why this derive doesn't implement [`convert::TypedParamName`] itself
///
/// This derive only ever generates [`convert::ToBytes`] for the annotated
/// struct — it does not implement [`convert::TypedParamName`] itself (that
/// trait additionally needs `type Value`, supplied by
/// [`hook_parameter!`](crate::hook_parameter)/
/// [`otxn_parameter!`](crate::otxn_parameter), which pairs the name type
/// with the value type it's read as — exactly like [`crate::hook_state!`]
/// pairs a [`HookKey`] type with its value type). `ParamName` (this derive)
/// and `TypedParamName` (that trait) share "name" in their names only in
/// the sense that both describe "the parameter name" concept from two
/// different angles (derive vs. runtime trait) — Rust's separate macro and
/// type namespaces mean this is not an identifier collision; the names are
/// kept parallel deliberately, for readability at the declaration site.
///
/// # Relationship to [`HookData`]
///
/// A hook-state value and a Hook API parameter *name* share the same
/// "fixed-offset struct" shape, but are genuinely different concepts —
/// `ParamName` is deliberately narrower than `HookData`, not just an alias
/// for it:
///
/// - A parameter name is only ever **written** (handed to
///   `hook_param`/`otxn_param` to locate a value) — never read back and
///   decoded as itself. `ParamName` reflects that by generating only
///   [`convert::ToBytes`]: no [`convert::FromBytes`], no
///   [`convert::FixedRead`], no inherent `LEN` const. Trying to read a
///   `#[derive(ParamName)]` type back as a value (or use it where
///   `hook_param_typed`/`otxn_param_typed` expect a value type) fails to compile
///   with an ordinary rustc trait-bound error naming the missing trait.
/// - A parameter name has its own length bound the Hook API itself
///   enforces — [`convert::PARAM_NAME_MAX_LEN`], **1 to 32 bytes**
///   (`hook_api.h`: `TOO_SMALL` below 1, `TOO_BIG` above 32) — distinct
///   from a hook state key's *fixed* 32 bytes, always zero-padded (see
///   [`HookKey`]), or a state *value*'s lack of any size cap at all.
///   `ParamName` checks this **at derive time**, unconditionally: a
///   `#[derive(ParamName)]` struct that encodes to 0 or to 33+ bytes fails
///   to compile at its own definition, before it's ever used as a
///   parameter name at all (contrast with [`HookKey`]'s analogous
///   derive-time check, which only has an upper bound — a key may be
///   shorter than 32 bytes, zero-padded, but a parameter name may not be
///   shorter than 1 byte).
///
/// # Grammar
///
/// Identical field grammar to [`HookData`] (see its doc comment): a plain,
/// non-generic, named-field struct, every field a fixed-size type
/// implementing [`convert::ToBytes`] (nesting another `#[derive(ParamName)]`
/// or `#[derive(HookData)]` struct as a field works the same way).
///
/// # Examples
///
/// A composite parameter name — a topic byte plus a sub-index, the same
/// idea `xahaud`'s own genesis governance hook uses for its `IS0`..`IS19`
/// seat parameters, expressed as a struct instead of a hand-built name:
///
/// ```
/// use hooks_lib::prelude::*;
/// use hooks_lib::{ParamName, ParamValue, otxn_parameter};
///
/// #[derive(ParamName, Clone, Copy)]
/// struct SeatParamName {
///     topic: u8,
///     seat: u8,
/// }
///
/// #[derive(ParamValue)]
/// struct Vote {
///     value: u8,
/// }
///
/// otxn_parameter!(SeatParamName => Vote);
///
/// let name = SeatParamName { topic: b'S', seat: 0 };
/// assert!(otxn_param_typed(&name).is_err());
/// ```
///
/// An enum, a tuple struct, and a generic struct are all rejected at
/// compile time, exactly like [`HookData`]:
///
/// ```compile_fail
/// use hooks_lib::ParamName;
///
/// #[derive(ParamName)]
/// enum NotAStruct {
///     A,
///     B,
/// }
/// ```
///
/// A struct that encodes to more than 32 bytes — the Hook API's own
/// parameter-name upper bound — is rejected **at its own definition**,
/// unlike an oversized `HookData` struct (which has no such bound at all —
/// only an oversized `HookKey` struct gets an analogous derive-time check):
///
/// ```compile_fail
/// use hooks_lib::ParamName;
///
/// #[derive(ParamName)]
/// struct TooBigForAParamName {
///     a: [u8; 20],
///     b: [u8; 20],
/// }
/// ```
///
/// A `#[derive(ParamName)]` type cannot be read back as a value — it has no
/// `FromBytes`/`FixedRead` impl, unlike [`HookData`]/[`ParamValue`]:
///
/// ```compile_fail
/// use hooks_lib::prelude::*;
/// use hooks_lib::ParamName;
///
/// #[derive(ParamName)]
/// struct SeatParamName {
///     topic: u8,
///     seat: u8,
/// }
///
/// // ERROR: `SeatParamName` has no `FixedRead` impl (`ParamName` never
/// // generates one — a parameter name is write-only).
/// let _: Result<SeatParamName> = otxn_param_exact(b"S");
/// ```
pub use hooks_macros::ParamName;

/// Derives [`convert::FromBytes`]/[`convert::FixedRead`] (only — no
/// [`convert::ToBytes`]) for a fixed-size, named-field struct used as a
/// **Hook API parameter value** — the `$Ty` in
/// [`hook_parameter!`](crate::hook_parameter)/
/// [`otxn_parameter!`](crate::otxn_parameter), read back and decoded by
/// [`api::hook_ctx::hook_param_typed`]/[`api::otxn::otxn_param_typed`] (and the
/// loose [`api::hook_ctx::hook_param_exact`]/
/// [`api::otxn::otxn_param_exact`]). See [`HookData`] for the analogous
/// hook-state *value* role, and [`ParamName`] for the parameter *name*
/// counterpart (what locates this value).
///
/// # Why this derive generates no [`convert::ToBytes`]
///
/// A parameter value is only ever **read back and decoded** — this hook
/// never writes its *own* parameters (`hook_param_set` writes a *different*
/// hook's parameter, taking a raw `&[u8]`, not a typed value). `ParamValue`
/// reflects that by generating only [`convert::FromBytes`]/
/// [`convert::FixedRead`]: no [`convert::ToBytes`], no inherent `LEN` const.
/// A consequence: a `#[derive(ParamValue)]` struct cannot be used as a
/// [`HookKey`]/[`ParamName`] field, nor as a hook-state value with
/// [`state::state_set_loose`] — both need `ToBytes`, which this derive
/// deliberately does not provide (use [`HookData`] for a struct that needs
/// to go both directions).
///
/// # Grammar
///
/// Identical field grammar to [`HookData`] (see its doc comment), except
/// every field's type need only implement [`convert::FromBytes`] (not also
/// [`convert::ToBytes`] — though every fixed-size type this crate provides
/// implements both, so this distinction rarely matters in practice).
///
/// # What gets generated
///
/// - `impl FromBytes for Name` / `impl FixedRead for Name`: fields are
///   decoded **back-to-back, in declaration order**, each consuming exactly
///   its own `<FieldType as ToBytes>::MAX_LEN` bytes — the same layout
///   [`HookData`] uses, just without the write-side impls or the `LEN`
///   const (there is no `ToBytes::MAX_LEN` on `Self` to name it by; the
///   per-field widths are summed directly in the generated code instead).
///
/// # Examples
///
/// A composite parameter value, paired with a plain byte-string name via
/// [`otxn_parameter!`](crate::otxn_parameter)'s two-argument form:
///
/// ```
/// use hooks_lib::prelude::*;
/// use hooks_lib::{ParamValue, otxn_parameter};
///
/// #[derive(ParamValue)]
/// struct Config {
///     min_amount: u64,
///     max_amount: u64,
/// }
///
/// struct CfgName;
/// otxn_parameter!(CfgName, b"CFG" => Config);
///
/// let cfg = otxn_param_typed(&CfgName);
/// assert_eq!(cfg.err(), Some(HookError::NotImplemented));
/// ```
///
/// An enum, a tuple struct, and a generic struct are all rejected at
/// compile time, exactly like [`HookData`]:
///
/// ```compile_fail
/// use hooks_lib::ParamValue;
///
/// #[derive(ParamValue)]
/// enum NotAStruct {
///     A,
///     B,
/// }
/// ```
///
/// A `#[derive(ParamValue)]` type cannot be used as a hook-state key — it
/// has no [`state::StateKeyEncode`] impl (nor even [`convert::ToBytes`]),
/// unlike [`HookKey`]:
///
/// ```compile_fail
/// use hooks_lib::prelude::*;
/// use hooks_lib::ParamValue;
///
/// #[derive(ParamValue)]
/// struct NotAKey {
///     value: u8,
/// }
///
/// // ERROR: `NotAKey` has no `StateKeyEncode` impl (`ParamValue` never
/// // generates one, nor the `ToBytes` impl such an encoding would need).
/// let _ = state_get::<u64>(&NotAKey { value: 0 });
/// ```
pub use hooks_macros::ParamValue;

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
    pub use crate::convert::{FixedRead, FromBytes, ToBytes, TypedParamName};
    pub use crate::error::{HookError, Result};
    pub use crate::state::{
        StateKeyEncode, TypedStateKey, state_foreign_get, state_foreign_get_typed,
        state_foreign_set_loose, state_foreign_set_typed, state_foreign_update_loose,
        state_foreign_update_typed, state_get, state_get_typed, state_set_loose, state_set_typed,
        state_update_loose, state_update_typed,
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
