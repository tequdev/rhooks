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

use crate::convert::{FromBytes, ToBytes};
use crate::error::{HookError, Result};
use crate::types::StateKey;

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
/// Implemented by [`crate::types::StateKey`] itself (identity — a raw,
/// already-32-byte key, e.g. one built with [`crate::pad!`], works directly
/// with the typed functions in this module) and by every enum the
/// [`state_keys!`](crate::state_keys) macro generates.
pub trait StateKeyEncode {
    /// The 32-byte state key `self` encodes to.
    fn encode(&self) -> StateKey;
}

impl StateKeyEncode for StateKey {
    #[inline(always)]
    fn encode(&self) -> StateKey {
        *self
    }
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

/// Write this hook's own state entry for `key`, encoding `value` as `T`.
/// Returns the number of bytes written.
#[inline(always)]
pub fn state_set_typed<T: ToBytes>(key: &impl StateKeyEncode, value: &T) -> Result<usize> {
    let encoded = key.encode();
    let raw = encode_write(value);
    let src = raw.get(..T::MAX_LEN).ok_or(HookError::TooBig)?;
    crate::api::state::state_set(src, &encoded)
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
}
