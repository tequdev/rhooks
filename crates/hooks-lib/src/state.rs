//! Typed hook state: [`state_get`]/[`state_set_typed`]/[`state_update_typed`]
//! (and their `_foreign` twins), built over [`mod@crate::api::state`]'s raw
//! caller-buffer functions and the [`crate::convert::ToBytes`]/
//! [`crate::convert::FromBytes`] traits, plus the
//! [`state_keys!`](crate::state_keys) macro for declaring a state-key enum.
//!
//! # This layer vs. `crate::api::state`'s single-value helpers
//!
//! [`mod@crate::api::state`] also has its own `state_u32`/`state_i64`/
//! `state_xfl`/`state_update_u64`/... family: small, fixed-shape
//! convenience wrappers over [`crate::api::state::state_exact`] for exactly
//! the primitive Rust integer/[`crate::xfl::XFL`] cases, each one a
//! standalone function with no key-type story of its own — the caller still
//! passes a raw `&[u8]` key. This module's [`state_get`]/[`state_set_typed`]/
//! [`state_update_typed`] instead work for *any* type implementing
//! [`crate::convert::ToBytes`]/[`crate::convert::FromBytes`] (every
//! `hooks_lib::types` newtype already does, and so does any hook-defined
//! type that implements the traits itself), and are meant to be paired with
//! [`state_keys!`](crate::state_keys) so the key itself is a typed enum
//! variant rather than a hand-built byte buffer. Reach for
//! `crate::api::state`'s helpers for a one-off primitive read/write; reach
//! for this module when a hook has more than a couple of distinct state
//! entries and wants the key space and value decoding both checked at
//! compile time.
//!
//! # Why `Ok(None)` for a missing entry
//!
//! [`crate::error::HookError::DoesntExist`] (`state`'s `-5`, "no entry for
//! this key") is mapped to `Ok(None)` rather than left as an `Err` variant a
//! caller must special-case on every read — the same shape as
//! `HashMap::get`/`BTreeMap::get`, where "absent" is completely ordinary,
//! not exceptional. Every *other* error — including a present-but-
//! undersized entry that fails to decode as `T` — still comes back through
//! `Err`, so a caller can never mistake a genuine decode failure for
//! "nothing was ever stored here."
//!
//! # `state_keys!`
//!
//! Declares an enum whose variants encode to fixed 32-byte
//! [`crate::types::StateKey`] values, for use with the functions above:
//!
//! ```
//! use hooks_lib::prelude::*;
//! use hooks_lib::state_keys;
//!
//! state_keys! {
//!     /// This hook's persistent data.
//!     enum DataKey {
//!         /// A running counter.
//!         Counter,
//!         /// A per-owner balance, keyed by the owner's account.
//!         Balance(AccountId),
//!     }
//! }
//!
//! // `NotImplemented` here is the host stub every Hook API call returns on
//! // a host build (see `hooks-core`) — this only proves the generated
//! // `encode()`/typed-storage call chain compiles and runs.
//! assert_eq!(
//!     state_get::<u64>(&DataKey::Counter),
//!     Err(HookError::NotImplemented)
//! );
//! ```
//!
//! Unit variants (`Counter` above) encode to "discriminant byte + zero
//! padding," entirely at compile time. Tuple variants (`Balance` above)
//! carry exactly one [`crate::convert::ToBytes`] payload, encoded at
//! runtime as "discriminant byte + payload + zero padding"; the macro
//! rejects (at compile time) a payload whose [`crate::convert::ToBytes::MAX_LEN`]
//! does not leave room for the discriminant byte in the 32-byte key.
//!
//! # Struct keys (`#[derive(crate::HookData)]`) vs. `state_keys!`
//!
//! [`state_keys!`](crate::state_keys) suits a **small, fixed set** of
//! distinct state entries: every variant is a separate, independently named
//! case (`Counter`, `Balance(AccountId)`, ...), each carrying at most one
//! [`crate::convert::ToBytes`] payload. A key that is itself a **composite of
//! several fields** — a tag byte plus an `AccountId` plus a `u32` sequence
//! number, say — doesn't fit that shape (a tuple variant takes exactly one
//! payload) and previously had to be hand-packed into a raw
//! [`crate::types::StateKey`] byte buffer.
//!
//! [`crate::HookData`] closes that gap: derive it on an ordinary named-field
//! struct (every field a fixed-size type — see its doc comment for the exact
//! grammar), and the struct becomes directly usable as a `state_get`/
//! `state_set_typed` key, via the blanket [`StateKeyEncode`] impl below —
//! `state_get(&MyKey { .. })` works with no `state_keys!` declaration at all.
//! The two are complementary, not competing: `state_keys!` for a handful of
//! named, independent key *cases*; a `#[derive(HookData)]` struct for one key
//! shape built out of several *fields* — and nothing stops a `state_keys!`
//! tuple variant's single payload from itself being a `HookData` struct, for
//! a hybrid of both.
//!
//! Any [`crate::convert::ToBytes`] type — not just a `HookData` struct — gets
//! [`StateKeyEncode`] for free this way, zero-padded up to the 32-byte key
//! space; a type whose [`crate::convert::ToBytes::MAX_LEN`] exceeds 32 fails
//! to compile as a key (the same monomorphized `const` assert pattern as
//! [`encode_write`]'s value-side check below), not silently truncate.
//!
//! # Pairing a key with its value type: [`TypedStateKey`]
//!
//! [`state_get`]/[`state_set_typed`]/[`state_update_typed`] take the key and
//! the value type `T` as *independent* generic parameters — nothing at the
//! type level stops calling `state_get::<WrongValue>(&key)` for a
//! `key`/`WrongValue` combination that was never meant to go together, as
//! long as `WrongValue: FromBytes` (true of nearly every fixed-size type
//! this crate provides — including, say, a *different* key's value type).
//! [`TypedStateKey`] closes that gap: implement it for a key type (directly,
//! or with the one-line [`hook_state!`](crate::hook_state) macro)
//! to declare its one paired value type once, then use
//! [`state_get_kv`]/[`state_set_kv`]/[`state_update_kv`] (+
//! `_foreign` twins) — these read `K::Value` off the key's own type, so a
//! mismatched value type has no generic parameter left to hide in; it's a
//! compile error instead of a latent bug. Prefer these whenever a key type
//! only ever pairs with one value type (every `HookData` key in practice).
//!
//! # Relationship to the `hook_param`/`otxn_param` typed layer
//!
//! [`crate::convert::ParamName`] is this module's counterpart for Hook API
//! parameters, deliberately shaped to *feel* the same even though the two
//! mechanisms aren't identical:
//!
//! | | this module (hook state) | [`crate::convert::ParamName`] (params) |
//! |---|---|---|
//! | declare the pairing | [`hook_state!`](crate::hook_state)`(Key => Value)` | [`hook_parameter!`](crate::hook_parameter)/[`otxn_parameter!`](crate::otxn_parameter)`(Ty => name)` |
//! | safe accessor(s) | `state_get_kv`/`state_set_kv`/`state_update_kv` | `hook_param_kv`/`otxn_param_kv` |
//! | loose escape hatch | [`state_get`]/[`state_set_typed`]/[`state_update_typed`] (independent `T`) | `hook_param_exact`/`otxn_param_exact` (independent `T`) |
//! | shared foundation | both built on [`crate::convert::ToBytes`]/[`crate::convert::FromBytes`]/[`crate::HookData`] — the same composite struct works as a state key/value *or* a param name/value |
//!
//! Both follow the identical shape: declare a pairing once, then call an
//! accessor that resolves the paired type from the key/name itself instead
//! of a second, independently-spelled generic parameter or argument — no
//! turbofish, no chance of a mismatch. Two real mechanism differences keep
//! them from going further than that:
//!
//! - **One key type, many key values, vs. one name value per type.** A
//!   state key type has many *runtime instances* (`DepositKey { tag: 1,
//!   owner: alice }`, `DepositKey { tag: 1, owner: bob }`, ... — a genuine
//!   key-value store), so [`TypedStateKey`] is a separate trait pairing a
//!   *key type* with a *value type*. A Hook API parameter is normally one
//!   compile-time-known name per type (`Config` always means `CFG`), so
//!   `ParamName` fuses "the name" (`Self::NAME`, a `const`) and "the
//!   value" (`Self`, read via [`crate::convert::FixedRead`]) into a
//!   *single* type — there's no separate "param key type" to speak of.
//! - **Read/write/update, vs. read-only.** Hook state is mutable and
//!   persisted, so this module has `_get`/`_set`/`_update` (+ `_foreign`
//!   twins) for both the loose and paired APIs. A `hook_param`/
//!   `otxn_param` is read-only from the reading hook's own perspective
//!   (`hook_param_set` writes a *different* hook's parameter, not this
//!   one) — so `ParamName`'s accessors are `_kv`-suffixed like this
//!   module's, but there is only ever a "get" shape, never a "set"/
//!   "update" one.
//!
//! Also see [`crate::HookData`]'s doc comment for the composite-struct
//! story both layers share, and [`crate::convert::ParamName`]'s doc
//! comment for the reciprocal comparison and its own zero-cost story
//! (parameter names have a cost dimension state keys don't: see that doc
//! comment's "Zero-cost" section).

use crate::convert::{FromBytes, ToBytes};
use crate::error::{HookError, Result};
use crate::types::{STATE_KEY_LEN, StateKey};

/// Maximum byte length of any value [`state_get`]/[`state_set_typed`]/
/// [`state_update_typed`] (and their `_foreign` twins) read or write.
///
/// 32, **not** picked to fit the largest type this crate provides
/// ([`crate::types::IouAmount`] is 48 bytes and does not fit) — picked
/// because it is the largest local `[0u8; N]` zero-init this toolchain's
/// wasm32v1-none codegen still lowers to a handful of inlined stores at
/// this crate's release profile (`opt-level = "z"`, `lto = "fat"`).
/// Beyond it (empirically, 34 bytes and up), rustc instead emits a call to
/// the shared `memset` builtin — a real, unguarded wasm `loop` that the
/// Hook API's guard checker rejects (see DESIGN.md §2 C2 and this crate's
/// convention of avoiding std idioms that lower to `memcpy`/`memset`
/// calls). Covers every fixed-size type this crate provides up to
/// [`crate::types::NameSpace`]/[`crate::types::Nonce`]/
/// [`crate::types::StateKey`]/[`crate::types::Hash`] (32 bytes); a hook
/// that needs a bigger typed value — [`crate::types::PublicKey`] (33),
/// [`crate::types::Keylet`] (34), [`crate::types::IouAmount`] (48), or a
/// custom type — should call [`crate::api::state`]'s raw, caller-buffer
/// functions directly instead of this module.
const MAX_TYPED_STATE_LEN: usize = 32;

/// Encodes a value into the fixed 32-byte hook-state key space.
///
/// Implemented by every enum the [`state_keys!`](crate::state_keys) macro
/// generates, and — via the blanket impl below — by every
/// [`crate::convert::ToBytes`] type, [`crate::types::StateKey`] itself
/// included (identity: a raw, already-32-byte key, e.g. one built with
/// [`crate::pad!`], works directly with the typed functions in this module).
/// See the module doc comment's "Struct keys" section for how a
/// [`crate::HookData`]-derived struct fits in.
pub trait StateKeyEncode {
    /// The 32-byte state key `self` encodes to.
    fn encode(&self) -> StateKey;
}

/// Blanket impl: any fixed-size [`crate::convert::ToBytes`] value can be
/// used directly as a state key, zero-padded up to
/// [`crate::types::STATE_KEY_LEN`] (32) bytes. A `T` whose
/// [`crate::convert::ToBytes::MAX_LEN`] exceeds 32 fails to compile — the
/// `const` assert is monomorphized per `T` and only fires when `encode` is
/// actually instantiated for that `T` (i.e., when it's actually used as a
/// key), mirroring [`encode_write`]'s identical value-side check.
impl<T: ToBytes> StateKeyEncode for T {
    #[inline(always)]
    fn encode(&self) -> StateKey {
        const {
            assert!(
                T::MAX_LEN <= STATE_KEY_LEN,
                "hooks_lib::state: T::MAX_LEN exceeds the 32-byte state key space"
            );
        }
        let mut raw = [0u8; STATE_KEY_LEN];
        let _ = self.write(&mut raw);
        StateKey::from(raw)
    }
}

/// A [`StateKeyEncode`] key type bound to exactly one value type.
///
/// [`state_get`]/[`state_set_typed`]/[`state_update_typed`] (and their
/// `_foreign` twins) take the key and the value type `T` as *independent*
/// generic parameters — nothing stops calling `state_get::<WrongValue>(&key)`
/// for a `key`/`WrongValue` pairing that was never intended, as long as
/// `WrongValue: FromBytes` (true of nearly every fixed-size type this crate
/// provides). Implementing `TypedStateKey` for a key type — directly, or via
/// [`hook_state!`](crate::hook_state) — ties it to exactly one
/// value type; [`state_get_kv`]/[`state_set_kv`]/
/// [`state_update_kv`] (+ `_foreign` twins) then read `K::Value` off
/// the key's own type, so there is no second, independently-chosen value
/// type left for a mismatch to hide in. Prefer these over the loose
/// `state_get`/`state_set_typed`/`state_update_typed` whenever a key type
/// only ever pairs with one value type — which is every `#[derive(HookData)]`
/// key, and every `state_keys!` variant that doesn't need to share its enum
/// with variants of differing value types.
pub trait TypedStateKey: StateKeyEncode {
    /// The one value type this key is paired with.
    type Value: ToBytes + FromBytes;
}

/// Shared read path for [`state_get`]/[`state_foreign_get`]: turns a raw
/// `state`/`state_foreign` `Result<usize>` (bytes written into `raw`) into a
/// decoded `Result<Option<T>>`, mapping
/// [`crate::error::HookError::DoesntExist`] to `Ok(None)` (see the module
/// doc comment). Factored out of the two public functions so the mapping
/// logic has one, directly testable, definition.
#[inline(always)]
fn decode_read<T: FromBytes>(
    result: Result<usize>,
    raw: &[u8; MAX_TYPED_STATE_LEN],
) -> Result<Option<T>> {
    match result {
        Ok(n) => {
            let src = raw.get(..n).ok_or(HookError::TooSmall)?;
            T::read(src).map(Some)
        }
        Err(HookError::DoesntExist) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Shared write path for [`state_set_typed`]/[`state_foreign_set_typed`]:
/// encodes `value` into a [`MAX_TYPED_STATE_LEN`]-byte scratch buffer.
///
/// A compile-time check (monomorphized per `T`) rejects any `T` whose
/// [`ToBytes::MAX_LEN`] does not fit — see [`MAX_TYPED_STATE_LEN`]'s doc
/// comment for the escape hatch. Without this check a too-large `T` would
/// silently encode to `0` bytes (`ToBytes::write`'s documented short-buffer
/// behavior) and write an empty state entry instead of failing loudly.
#[inline(always)]
fn encode_write<T: ToBytes>(value: &T) -> [u8; MAX_TYPED_STATE_LEN] {
    const {
        assert!(
            T::MAX_LEN <= MAX_TYPED_STATE_LEN,
            "hooks_lib::state: T::MAX_LEN exceeds the typed-storage buffer \
             — use api::state's raw functions directly for larger values"
        );
    }
    let mut raw = [0u8; MAX_TYPED_STATE_LEN];
    let _ = value.write(&mut raw);
    raw
}

/// Read this hook's own state entry for `key`, decoded as `T`.
///
/// `Ok(None)` means no entry exists for `key` — see the module doc comment.
#[inline(always)]
pub fn state_get<T: FromBytes>(key: &impl StateKeyEncode) -> Result<Option<T>> {
    let encoded = key.encode();
    let mut raw = [0u8; MAX_TYPED_STATE_LEN];
    let result = crate::api::state::state(&mut raw, &encoded);
    decode_read(result, &raw)
}

/// Read this hook's own state entry for `key`, decoded as `key`'s own
/// [`TypedStateKey::Value`] — the key/value-pairing-safe counterpart to
/// [`state_get`] (see [`TypedStateKey`]'s doc comment for why). `Ok(None)`
/// means no entry exists — see the module doc comment.
#[inline(always)]
pub fn state_get_kv<K: TypedStateKey>(key: &K) -> Result<Option<K::Value>> {
    state_get::<K::Value>(key)
}

/// Write this hook's own state entry for `key`, encoding `value` as `T`.
/// Returns the number of bytes written.
#[inline(always)]
pub fn state_set_typed<T: ToBytes>(key: &impl StateKeyEncode, value: &T) -> Result<usize> {
    let encoded = key.encode();
    let raw = encode_write(value);
    let src = raw.get(..T::MAX_LEN).ok_or(HookError::TooBig)?;
    crate::api::state::state_set(src, &encoded)
}

/// Write this hook's own state entry for `key`, encoding `value` as `key`'s
/// own [`TypedStateKey::Value`] — the key/value-pairing-safe counterpart to
/// [`state_set_typed`] (see [`TypedStateKey`]'s doc comment for why):
/// `value`'s type is checked against `K::Value` at the call site, so
/// passing a value meant for a different key is a compile error. Returns
/// the number of bytes written.
#[inline(always)]
pub fn state_set_kv<K: TypedStateKey>(key: &K, value: &K::Value) -> Result<usize> {
    state_set_typed(key, value)
}

/// Read-modify-write this hook's own state entry for `key`: reads the
/// current value (or `None` if absent), calls `f` to compute the next
/// value, writes it back, and returns the number of bytes written.
#[inline(always)]
pub fn state_update_typed<T, F>(key: &impl StateKeyEncode, f: F) -> Result<usize>
where
    T: FromBytes + ToBytes,
    F: FnOnce(Option<T>) -> T,
{
    let current = state_get::<T>(key)?;
    let next = f(current);
    state_set_typed(key, &next)
}

/// Read-modify-write this hook's own state entry for `key`, using `key`'s
/// own [`TypedStateKey::Value`] — the key/value-pairing-safe counterpart to
/// [`state_update_typed`] (see [`TypedStateKey`]'s doc comment for why).
#[inline(always)]
pub fn state_update_kv<K, F>(key: &K, f: F) -> Result<usize>
where
    K: TypedStateKey,
    F: FnOnce(Option<K::Value>) -> K::Value,
{
    state_update_typed(key, f)
}

/// Read a state entry belonging to another namespace/account, decoded as
/// `T`. `namespace`/`account` follow
/// [`crate::api::state::state_foreign`]'s `Option` convention. `Ok(None)`
/// means no entry exists — see the module doc comment.
#[inline(always)]
pub fn state_foreign_get<T: FromBytes>(
    key: &impl StateKeyEncode,
    namespace: Option<&[u8]>,
    account: Option<&[u8]>,
) -> Result<Option<T>> {
    let encoded = key.encode();
    let mut raw = [0u8; MAX_TYPED_STATE_LEN];
    let result = crate::api::state::state_foreign(&mut raw, &encoded, namespace, account);
    decode_read(result, &raw)
}

/// Write a state entry belonging to another namespace/account, encoding
/// `value` as `T`. `namespace`/`account` follow
/// [`crate::api::state::state_foreign`]'s `Option` convention. Returns the
/// number of bytes written.
#[inline(always)]
pub fn state_foreign_set_typed<T: ToBytes>(
    key: &impl StateKeyEncode,
    value: &T,
    namespace: Option<&[u8]>,
    account: Option<&[u8]>,
) -> Result<usize> {
    let encoded = key.encode();
    let raw = encode_write(value);
    let src = raw.get(..T::MAX_LEN).ok_or(HookError::TooBig)?;
    crate::api::state::state_foreign_set(src, &encoded, namespace, account)
}

/// Read-modify-write a state entry belonging to another namespace/account:
/// reads the current value (or `None` if absent), calls `f` to compute the
/// next value, writes it back, and returns the number of bytes written.
/// `namespace`/`account` follow
/// [`crate::api::state::state_foreign`]'s `Option` convention.
#[inline(always)]
pub fn state_foreign_update_typed<T, F>(
    key: &impl StateKeyEncode,
    namespace: Option<&[u8]>,
    account: Option<&[u8]>,
    f: F,
) -> Result<usize>
where
    T: FromBytes + ToBytes,
    F: FnOnce(Option<T>) -> T,
{
    let current = state_foreign_get::<T>(key, namespace, account)?;
    let next = f(current);
    state_foreign_set_typed(key, &next, namespace, account)
}

/// Read a state entry belonging to another namespace/account, decoded as
/// `key`'s own [`TypedStateKey::Value`] — the key/value-pairing-safe
/// counterpart to [`state_foreign_get`] (see [`TypedStateKey`]'s doc
/// comment for why). `namespace`/`account` follow
/// [`crate::api::state::state_foreign`]'s `Option` convention. `Ok(None)`
/// means no entry exists — see the module doc comment.
#[inline(always)]
pub fn state_foreign_get_kv<K: TypedStateKey>(
    key: &K,
    namespace: Option<&[u8]>,
    account: Option<&[u8]>,
) -> Result<Option<K::Value>> {
    state_foreign_get::<K::Value>(key, namespace, account)
}

/// Write a state entry belonging to another namespace/account, encoding
/// `value` as `key`'s own [`TypedStateKey::Value`] — the
/// key/value-pairing-safe counterpart to [`state_foreign_set_typed`] (see
/// [`TypedStateKey`]'s doc comment for why). `namespace`/`account` follow
/// [`crate::api::state::state_foreign`]'s `Option` convention. Returns the
/// number of bytes written.
#[inline(always)]
pub fn state_foreign_set_kv<K: TypedStateKey>(
    key: &K,
    value: &K::Value,
    namespace: Option<&[u8]>,
    account: Option<&[u8]>,
) -> Result<usize> {
    state_foreign_set_typed(key, value, namespace, account)
}

/// Read-modify-write a state entry belonging to another namespace/account,
/// using `key`'s own [`TypedStateKey::Value`] — the key/value-pairing-safe
/// counterpart to [`state_foreign_update_typed`] (see [`TypedStateKey`]'s
/// doc comment for why). `namespace`/`account` follow
/// [`crate::api::state::state_foreign`]'s `Option` convention.
#[inline(always)]
pub fn state_foreign_update_kv<K, F>(
    key: &K,
    namespace: Option<&[u8]>,
    account: Option<&[u8]>,
    f: F,
) -> Result<usize>
where
    K: TypedStateKey,
    F: FnOnce(Option<K::Value>) -> K::Value,
{
    state_foreign_update_typed(key, namespace, account, f)
}

/// Declares an enum whose variants encode to fixed 32-byte
/// [`crate::types::StateKey`] values, implementing [`StateKeyEncode`] for
/// it. See the module doc comment for the encoding rules and an example.
///
/// Grammar: unit variants (`Name`) and single-payload tuple variants
/// (`Name(PayloadType)`, `PayloadType: `[`crate::convert::ToBytes`]) may be
/// freely mixed; every variant is assigned a sequential `u8` discriminant
/// by this macro (kept separate from the generated enum's own, ordinary
/// Rust discriminants, since a data-carrying variant cannot have one on
/// stable Rust) — declaration order is significant, and inserting or
/// reordering a variant changes every later variant's encoded key.
#[macro_export]
macro_rules! state_keys {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $Name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident $(($payload:ty))?
            ),* $(,)?
        }
    ) => {
        $crate::__state_keys_step! {
            @step
            meta = [$(#[$enum_meta])*], vis = $vis, name = $Name,
            fields = [ $( $(#[$variant_meta])* $variant $(($payload))? ),* ],
            next = 0u8,
            enum_body = [],
            arms = [],
            discs = [],
            fits_checks = []
        }
    };
}

/// Internal recursive tt-muncher backing [`state_keys!`](crate::state_keys).
///
/// `#[doc(hidden)]` but necessarily `#[macro_export]`ed (a macro invoked as
/// `$crate::name!` from another macro's expansion must be exported) —
/// mirrors `txn.rs`'s `__txn_template_step!` split (public entry macro,
/// hidden recursive worker).
///
/// Peels one variant off `fields` per step, appending a complete, already
/// concrete `enum_body`/`arms`/`discs`/`fits_checks` entry for it — the
/// unit-variant and single-payload-tuple-variant cases each get their own
/// matcher arm below, so at accumulation time every `$variant`/`$payload`
/// is a *singular* bound value (not a repetition), and each generated
/// `arms` entry is a complete, self-contained `pattern => body` unit. This
/// sidesteps two dead ends: (1) a macro invocation cannot expand to a bare
/// match arm (Rust: "macros cannot expand to match arms") — every
/// `Name::Variant => { .. }` here is written out whole, by one macro step,
/// not spliced together from a separate pattern-producing and
/// body-producing call; (2) transcribing a *conditionally shaped* pattern
/// (`Name::Variant` vs. `Name::Variant(__payload)`) via a single
/// `$(...)? `-gated group inside one repetition requires that group to
/// itself reference the metavariable driving the optionality, which a bare
/// `(__payload)` does not — dispatching unit vs. tuple to separate matcher
/// arms avoids needing that trick at all.
#[doc(hidden)]
#[macro_export]
macro_rules! __state_keys_step {
    // Terminal: every variant has been consumed — emit the enum, the
    // `StateKeyEncode` impl, and the compile-time checks.
    (
        @step
        meta = [$($enum_meta:tt)*], vis = $vis:vis, name = $Name:ident,
        fields = [],
        next = $next:expr,
        enum_body = [$($enum_body:tt)*],
        arms = [$($arms:tt)*],
        discs = [$($discs:tt)*],
        fits_checks = [$($fits_checks:tt)*]
    ) => {
        $($enum_meta)*
        $vis enum $Name {
            $($enum_body)*
        }

        impl $crate::state::StateKeyEncode for $Name {
            #[inline(always)]
            fn encode(&self) -> $crate::types::StateKey {
                match self {
                    $($arms)*
                }
            }
        }

        // Every payload must leave room for the 1-byte discriminant in the
        // fixed 32-byte key.
        $($fits_checks)*

        // Discriminants must be pairwise distinct.
        #[allow(clippy::indexing_slicing)] // const-evaluated only, bounded by the `while` guards
        const _: () = {
            const DISCS: &[u8] = &[$($discs)*];
            let mut i = 0;
            while i < DISCS.len() {
                let mut j = i.wrapping_add(1);
                while j < DISCS.len() {
                    assert!(DISCS[i] != DISCS[j], "state_keys!: duplicate discriminant");
                    j = j.wrapping_add(1);
                }
                i = i.wrapping_add(1);
            }
        };
    };

    // Unit variant.
    (
        @step
        meta = [$($enum_meta:tt)*], vis = $vis:vis, name = $Name:ident,
        fields = [
            $(#[$variant_meta:meta])* $variant:ident
            $(, $($rest:tt)*)?
        ],
        next = $next:expr,
        enum_body = [$($enum_body:tt)*],
        arms = [$($arms:tt)*],
        discs = [$($discs:tt)*],
        fits_checks = [$($fits_checks:tt)*]
    ) => {
        $crate::__state_keys_step! {
            @step
            meta = [$($enum_meta)*], vis = $vis, name = $Name,
            fields = [ $($($rest)*)? ],
            next = ($next + 1u8),
            enum_body = [
                $($enum_body)*
                $(#[$variant_meta])* $variant,
            ],
            arms = [
                $($arms)*
                $Name::$variant => {
                    let mut __out = [0u8; $crate::types::STATE_KEY_LEN];
                    if let Some(__byte) = __out.get_mut(0) {
                        *__byte = $next;
                    }
                    $crate::types::StateKey::from(__out)
                }
            ],
            discs = [ $($discs)* $next, ],
            fits_checks = [ $($fits_checks)* ]
        }
    };

    // Single-payload tuple variant.
    (
        @step
        meta = [$($enum_meta:tt)*], vis = $vis:vis, name = $Name:ident,
        fields = [
            $(#[$variant_meta:meta])* $variant:ident ($payload:ty)
            $(, $($rest:tt)*)?
        ],
        next = $next:expr,
        enum_body = [$($enum_body:tt)*],
        arms = [$($arms:tt)*],
        discs = [$($discs:tt)*],
        fits_checks = [$($fits_checks:tt)*]
    ) => {
        $crate::__state_keys_step! {
            @step
            meta = [$($enum_meta)*], vis = $vis, name = $Name,
            fields = [ $($($rest)*)? ],
            next = ($next + 1u8),
            enum_body = [
                $($enum_body)*
                $(#[$variant_meta])* $variant($payload),
            ],
            arms = [
                $($arms)*
                $Name::$variant(__payload) => {
                    let mut __out = [0u8; $crate::types::STATE_KEY_LEN];
                    if let Some(__byte) = __out.get_mut(0) {
                        *__byte = $next;
                    }
                    if let Some(__rest) = __out.get_mut(1..) {
                        let _ = <$payload as $crate::convert::ToBytes>::write(
                            __payload, __rest,
                        );
                    }
                    $crate::types::StateKey::from(__out)
                }
            ],
            discs = [ $($discs)* $next, ],
            fits_checks = [
                $($fits_checks)*
                const _: () = assert!(
                    <$payload as $crate::convert::ToBytes>::MAX_LEN
                        < $crate::types::STATE_KEY_LEN,
                    "state_keys!: payload too large to leave room for the discriminant byte in a 32-byte key"
                );
            ]
        }
    };
}

/// Implements [`TypedStateKey`] for `$Key`, pairing it with `$Value` — the
/// one-line way to opt a key type into [`state_get_kv`]/
/// [`state_set_kv`]/[`state_update_kv`] (+ `_foreign` twins).
/// See [`TypedStateKey`]'s doc comment for why these are safer than the
/// loose `state_get`/`state_set_typed`/`state_update_typed`.
///
/// ```
/// use hooks_lib::prelude::*;
/// use hooks_lib::{hook_state, HookData};
///
/// #[derive(HookData, Clone, Copy)]
/// struct MyKey {
///     tag: u8,
/// }
///
/// #[derive(HookData, Clone, Copy, Debug, PartialEq)]
/// struct MyValue {
///     count: u32,
/// }
///
/// hook_state!(MyKey => MyValue);
///
/// // `NotImplemented` here is the host stub every Hook API call returns on
/// // a host build — this only proves the generated `TypedStateKey`/
/// // `state_get_kv` call chain compiles and runs.
/// assert_eq!(
///     state_get_kv(&MyKey { tag: 0 }),
///     Err(HookError::NotImplemented)
/// );
/// ```
#[macro_export]
macro_rules! hook_state {
    ($Key:ty => $Value:ty) => {
        impl $crate::state::TypedStateKey for $Key {
            type Value = $Value;
        }
    };
}

#[cfg(test)]
mod tests {
    // Tests are exempt from the panic-freedom lints (see docs/DESIGN.md
    // §8); indexing on known-good, fixed-size local arrays is idiomatic
    // here (matches the convention in `txn.rs`'s test module).
    #![allow(clippy::indexing_slicing)]

    use super::*;
    use crate::error::HookError;
    use crate::types::STATE_KEY_LEN;

    #[test]
    fn state_get_maps_doesnt_exist_to_none() {
        let raw = [0u8; MAX_TYPED_STATE_LEN];
        assert_eq!(
            decode_read::<u32>(Err(HookError::DoesntExist), &raw),
            Ok(None)
        );
    }

    #[test]
    fn state_get_propagates_other_errors() {
        let raw = [0u8; MAX_TYPED_STATE_LEN];
        assert_eq!(
            decode_read::<u32>(Err(HookError::InternalError), &raw),
            Err(HookError::InternalError)
        );
    }

    #[test]
    fn state_get_decodes_present_value() {
        let mut raw = [0u8; MAX_TYPED_STATE_LEN];
        raw[0] = 42;
        assert_eq!(decode_read::<u32>(Ok(4), &raw), Ok(Some(42u32)));
    }

    #[test]
    fn state_get_propagates_short_decode_as_error_not_none() {
        // 3 bytes written is not enough for a `u32` (needs 4): this must
        // surface as an `Err`, never be confused with "absent."
        let raw = [0u8; MAX_TYPED_STATE_LEN];
        assert_eq!(decode_read::<u32>(Ok(3), &raw), Err(HookError::TooSmall));
    }

    #[test]
    fn encode_write_round_trips_through_from_bytes() {
        let raw = encode_write(&0x1122_3344u32);
        assert_eq!(u32::read(&raw), Ok(0x1122_3344));
    }

    #[test]
    fn smoke_not_implemented_on_host() {
        assert_eq!(
            state_get::<u32>(&StateKey::from([0u8; STATE_KEY_LEN])),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            state_set_typed(&StateKey::from([0u8; STATE_KEY_LEN]), &1u32),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            state_update_typed(&StateKey::from([0u8; STATE_KEY_LEN]), |_: Option<u32>| 1u32),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            state_foreign_get::<u32>(&StateKey::from([0u8; STATE_KEY_LEN]), None, None),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            state_foreign_set_typed(&StateKey::from([0u8; STATE_KEY_LEN]), &1u32, None, None),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            state_foreign_update_typed(
                &StateKey::from([0u8; STATE_KEY_LEN]),
                None,
                None,
                |_: Option<u32>| 1u32
            ),
            Err(HookError::NotImplemented)
        );
    }

    state_keys! {
        /// Test-only key space exercising every `state_keys!` variant shape.
        enum TestKey {
            /// Unit variant.
            Counter,
            /// Tuple variant with a fixed-size payload.
            Balance(u32),
        }
    }

    #[test]
    fn unit_variant_encodes_discriminant_and_zero_pad() {
        let mut expected = [0u8; STATE_KEY_LEN];
        expected[0] = 0;
        assert_eq!(TestKey::Counter.encode(), StateKey::from(expected));
    }

    #[test]
    fn tuple_variant_encodes_discriminant_payload_and_zero_pad() {
        let mut expected = [0u8; STATE_KEY_LEN];
        expected[0] = 1;
        expected[1..5].copy_from_slice(&0x0102_0304u32.to_le_bytes());
        assert_eq!(
            TestKey::Balance(0x0102_0304).encode(),
            StateKey::from(expected)
        );
    }

    #[test]
    fn distinct_variants_encode_to_distinct_keys() {
        assert_ne!(TestKey::Counter.encode(), TestKey::Balance(0).encode());
    }

    // `TypedStateKey`/`hook_state!`: a key type paired with exactly one
    // value type, via the `_kv`-suffixed functions (see their doc comments).
    hook_state!(TestKey => u32);

    #[test]
    fn typed_pair_smoke_not_implemented_on_host() {
        assert_eq!(
            state_get_kv(&TestKey::Counter),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            state_set_kv(&TestKey::Counter, &1u32),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            state_update_kv(&TestKey::Counter, |_| 1u32),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            state_foreign_get_kv(&TestKey::Counter, None, None),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            state_foreign_set_kv(&TestKey::Counter, &1u32, None, None),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            state_foreign_update_kv(&TestKey::Counter, None, None, |_| 1u32),
            Err(HookError::NotImplemented)
        );
    }
}
