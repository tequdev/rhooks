//! Fixed-size protocol buffer aliases.
//!
//! All of these are always zero-initialized as `[0u8; N]` at call sites —
//! never via `MaybeUninit` (see `macros.rs` for why `uninit_buf!` is
//! deliberately not provided).

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

/// A 20-byte AccountID.
pub type AccountId = [u8; ACC_ID_LEN];
/// A 32-byte hash (transaction ID, ledger hash, ...).
pub type Hash = [u8; HASH_LEN];
/// A 34-byte Keylet.
pub type Keylet = [u8; KEYLET_LEN];
/// A 32-byte hook state key.
pub type StateKey = [u8; STATE_KEY_LEN];
/// A 32-byte hook state namespace.
pub type NameSpace = [u8; NAMESPACE_LEN];
/// A 32-byte nonce.
pub type Nonce = [u8; NONCE_LEN];
/// A 33-byte public key.
pub type PublicKey = [u8; PUB_KEY_LEN];
/// A 20-byte currency code.
pub type CurrencyCode = [u8; CURRENCY_CODE_LEN];
/// An 8-byte serialized native (XRP/XAH) amount.
pub type NativeAmount = [u8; NATIVE_AMOUNT_LEN];
/// A 48-byte serialized IOU amount.
pub type IouAmount = [u8; IOU_AMOUNT_LEN];
