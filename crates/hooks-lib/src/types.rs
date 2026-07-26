//! Fixed-size protocol buffer newtypes.
//!
//! Each type below (`AccountId`, `Hash`, `Keylet`, ...) is a
//! `#[repr(transparent)]` tuple struct wrapping a `[u8; N]` — not a bare
//! type alias. `#[repr(transparent)]` guarantees the wrapper has *exactly*
//! the same layout, size, and alignment as its inner array (zero-cost: no
//! extra memory, no extra indirection, and it is FFI-compatible with a raw
//! `[u8; N]` wherever that matters), so this newtype step only adds
//! type-level distinctness (an `AccountId` and a `Hash` can no longer be
//! passed to each other's slots by accident) at zero runtime cost.
//!
//! The inner field is `pub` (`AccountId(pub [u8; 20])`) and every type
//! implements [`core::ops::Deref`]/[`core::ops::DerefMut`] (target
//! `[u8; N]`), [`AsRef<[u8]>`]/[`AsMut<[u8]>`], and
//! `From<[u8; N]>`/`Into<[u8; N]>`, to keep migration cost low: method
//! calls (`.as_ptr()`, `.len()`, indexing, `.starts_with(..)`, ...) reach
//! through to the inner array via auto-deref exactly as they did when
//! these were plain array aliases. The one place that *does* need an
//! explicit conversion is passing a newtype by reference where a bare
//! `&[u8]`/`&mut [u8]` parameter is expected (Rust does not chain a
//! user `Deref` impl with the built-in array-to-slice unsized coercion at
//! a call site) — write `value.as_ref()` / `value.as_mut()` there.
//! [`mod@crate::api::state`]'s key parameters are the deliberate exception:
//! `state`/`state_set`/`state_foreign(_set)` bound their `key` parameter by
//! `AsRef<[u8]>` instead of taking a bare `&[u8]`, specifically so
//! `state(&mut raw, &STATE_KEY)` works with no `.as_ref()` at the call site
//! (see that module's doc comment for why, and why `namespace`/`account`
//! don't get the same treatment).
//!
//! Every type here also implements [`crate::convert::ToBytes`]/
//! [`crate::convert::FromBytes`] (a fixed-length passthrough to/from its
//! inner array), so all ten work directly as
//! [`crate::state::state_get`]/[`crate::state::state_set_typed`] value
//! types.
//!
//! All of these are always zero-initialized as `[0u8; N]` at call sites —
//! never via `MaybeUninit` (see `macros.rs` for why `uninit_buf!` is
//! deliberately not provided).

use crate::convert::{FromBytes, ToBytes};
use crate::error::Result;

/// Length in bytes of an [`AccountId`].
pub const ACC_ID_LEN: usize = 20;
/// Length in bytes of a [`Hash`].
pub const HASH_LEN: usize = 32;
/// Length in bytes of a [`Keylet`].
pub const KEYLET_LEN: usize = 34;
/// Length in bytes of a [`StateKey`].
pub const STATE_KEY_LEN: usize = 32;
/// Length in bytes of a [`NameSpace`].
pub const NAMESPACE_LEN: usize = 32;
/// Length in bytes of a [`Nonce`].
pub const NONCE_LEN: usize = 32;
/// Length in bytes of a [`PublicKey`].
pub const PUB_KEY_LEN: usize = 33;
/// Length in bytes of a [`CurrencyCode`].
pub const CURRENCY_CODE_LEN: usize = 20;
/// Length in bytes of a [`NativeAmount`].
pub const NATIVE_AMOUNT_LEN: usize = 8;
/// Length in bytes of an [`IouAmount`].
pub const IOU_AMOUNT_LEN: usize = 48;
/// Maximum length in bytes of a serialized `EmitDetails` object
/// (`etxn_details` output): 138 bytes when this hook's wasm module exports
/// a `cbak` callback, 116 bytes otherwise (see xahaud's
/// `HookAPI::etxn_details`, `src/xrpld/app/hook/detail/HookAPI.cpp`).
/// `etxn_details` is a caller-buffer/returned-length API (like
/// [`crate::api::hook_ctx::hook_param`]) — size a buffer to this constant
/// and trust the returned length; do not assume it is always fully written.
pub const EMIT_DETAILS_MAX_LEN: usize = 138;

/// Defines one `#[repr(transparent)]` fixed-size buffer newtype, plus its
/// `Deref`/`DerefMut`/`AsRef`/`AsMut`/`From`/`Default`/`ToBytes`/`FromBytes`
/// impls. See the module doc comment for the rationale.
macro_rules! fixed_bytes_type {
    ($(#[$meta:meta])* $name:ident, $len:expr) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $name(pub [u8; $len]);

        impl core::ops::Deref for $name {
            type Target = [u8; $len];

            #[inline(always)]
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl core::ops::DerefMut for $name {
            #[inline(always)]
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }

        impl AsRef<[u8]> for $name {
            #[inline(always)]
            fn as_ref(&self) -> &[u8] {
                &self.0
            }
        }

        impl AsMut<[u8]> for $name {
            #[inline(always)]
            fn as_mut(&mut self) -> &mut [u8] {
                &mut self.0
            }
        }

        impl From<[u8; $len]> for $name {
            #[inline(always)]
            fn from(value: [u8; $len]) -> Self {
                $name(value)
            }
        }

        impl From<$name> for [u8; $len] {
            #[inline(always)]
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl Default for $name {
            #[inline(always)]
            fn default() -> Self {
                $name([0u8; $len])
            }
        }

        impl ToBytes for $name {
            const MAX_LEN: usize = $len;

            #[inline(always)]
            fn write(&self, buf: &mut [u8]) -> usize {
                self.0.write(buf)
            }
        }

        impl FromBytes for $name {
            #[inline(always)]
            fn read(buf: &[u8]) -> Result<Self> {
                <[u8; $len]>::read(buf).map($name)
            }
        }
    };
}

fixed_bytes_type!(
    /// A 20-byte AccountID.
    AccountId,
    ACC_ID_LEN
);
fixed_bytes_type!(
    /// A 32-byte hash (transaction ID, ledger hash, ...).
    Hash,
    HASH_LEN
);
fixed_bytes_type!(
    /// A 34-byte Keylet.
    Keylet,
    KEYLET_LEN
);
fixed_bytes_type!(
    /// A 32-byte hook state key.
    StateKey,
    STATE_KEY_LEN
);
fixed_bytes_type!(
    /// A 32-byte hook state namespace.
    NameSpace,
    NAMESPACE_LEN
);
fixed_bytes_type!(
    /// A 32-byte nonce.
    Nonce,
    NONCE_LEN
);
fixed_bytes_type!(
    /// A 33-byte public key.
    PublicKey,
    PUB_KEY_LEN
);
fixed_bytes_type!(
    /// A 20-byte currency code.
    CurrencyCode,
    CURRENCY_CODE_LEN
);
fixed_bytes_type!(
    /// An 8-byte serialized native (XRP/XAH) amount.
    NativeAmount,
    NATIVE_AMOUNT_LEN
);
fixed_bytes_type!(
    /// A 48-byte serialized IOU amount.
    IouAmount,
    IOU_AMOUNT_LEN
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deref_reaches_inner_array_methods() {
        let id = AccountId([0xAB; ACC_ID_LEN]);
        assert_eq!(id.len(), ACC_ID_LEN);
        assert!(id.starts_with(&[0xAB]));
    }

    #[test]
    fn as_ref_as_mut_round_trip() {
        let mut id = AccountId([0u8; ACC_ID_LEN]);
        id.as_mut().copy_from_slice(&[7u8; ACC_ID_LEN]);
        assert_eq!(id.as_ref(), &[7u8; ACC_ID_LEN]);
    }

    #[test]
    fn from_into_array_round_trip() {
        let arr = [9u8; ACC_ID_LEN];
        let id = AccountId::from(arr);
        let back: [u8; ACC_ID_LEN] = id.into();
        assert_eq!(back, arr);
    }

    #[test]
    fn default_is_all_zero() {
        assert_eq!(AccountId::default(), AccountId([0u8; ACC_ID_LEN]));
    }

    #[test]
    fn to_bytes_from_bytes_round_trip() {
        let id = AccountId([5u8; ACC_ID_LEN]);
        let mut buf = [0u8; ACC_ID_LEN];
        assert_eq!(id.write(&mut buf), ACC_ID_LEN);
        assert_eq!(AccountId::read(&buf), Ok(id));
    }

    #[test]
    fn repr_transparent_matches_inner_array_size() {
        assert_eq!(core::mem::size_of::<AccountId>(), ACC_ID_LEN);
        assert_eq!(
            core::mem::align_of::<AccountId>(),
            core::mem::align_of::<[u8; ACC_ID_LEN]>()
        );
    }
}
