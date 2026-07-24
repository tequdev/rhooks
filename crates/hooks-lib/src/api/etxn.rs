//! Emitted-transaction (`etxn_*`) API: reserving emission slots, computing
//! fees/nonces, and emitting transactions.
//!
//! Burden, fee, and generation values are naturally unsigned magnitudes even
//! though the Hook API wire type is `i64` — this module returns them as
//! `u64` (the non-negative `i64` payload cast with `as`, safe because
//! [`crate::error::res`] already rejected negative values).

use crate::error::{Result, res};
use crate::types::{HASH_LEN, Hash, NONCE_LEN, Nonce};

/// Burden of this hook's own emitted transactions so far.
#[inline(always)]
pub fn etxn_burden() -> Result<u64> {
    res(unsafe { hooks_core::etxn_burden() }).map(|v| v as u64)
}

/// Writes the serialized `EmitDetails` object for the next transaction this
/// hook would emit into `out`, returning the number of bytes written.
///
/// The length is not protocol-fixed: it depends on whether this hook's wasm
/// module exports a `cbak` callback (xahaud's `HookAPI::etxn_details`,
/// `src/xrpld/app/hook/detail/HookAPI.cpp`, appends an extra `sfEmitCallback`
/// field when it does) — 116 bytes without a callback, 138 bytes with one.
/// Size `out` to [`crate::types::EMIT_DETAILS_MAX_LEN`] (the worst case) and
/// trust the returned length, not `out.len()`, as the field's true size.
#[inline(always)]
pub fn etxn_details(out: &mut [u8]) -> Result<usize> {
    res(unsafe { hooks_core::etxn_details(out.as_mut_ptr() as u32, out.len() as u32) })
        .map(|v| v as usize)
}

/// The base fee (in drops) required to emit `tx_blob`.
#[inline(always)]
pub fn etxn_fee_base(tx_blob: &[u8]) -> Result<u64> {
    res(unsafe { hooks_core::etxn_fee_base(tx_blob.as_ptr() as u32, tx_blob.len() as u32) })
        .map(|v| v as u64)
}

/// Reserve `count` emission slots for this hook invocation. Must be called
/// before [`emit`].
#[inline(always)]
pub fn etxn_reserve(count: u32) -> Result<i64> {
    res(unsafe { hooks_core::etxn_reserve(count) })
}

/// The generation of transactions emitted by this hook so far.
#[inline(always)]
pub fn etxn_generation() -> Result<u64> {
    res(unsafe { hooks_core::etxn_generation() }).map(|v| v as u64)
}

/// A fresh nonce for use in an emitted transaction.
#[inline(always)]
pub fn etxn_nonce() -> Result<Nonce> {
    let mut buf: Nonce = [0u8; NONCE_LEN];
    res(unsafe { hooks_core::etxn_nonce(buf.as_mut_ptr() as u32, buf.len() as u32) })?;
    Ok(buf)
}

/// Emit `tx_blob` as a new transaction. Requires a prior [`etxn_reserve`]
/// call. Returns the emitted transaction's hash.
#[inline(always)]
pub fn emit(tx_blob: &[u8]) -> Result<Hash> {
    let mut buf: Hash = [0u8; HASH_LEN];
    res(unsafe {
        hooks_core::emit(
            buf.as_mut_ptr() as u32,
            buf.len() as u32,
            tx_blob.as_ptr() as u32,
            tx_blob.len() as u32,
        )
    })?;
    Ok(buf)
}

/// Prepare a transaction template (`template`) into `out`, substituting
/// hook-computed fields. Returns the number of bytes written.
#[inline(always)]
pub fn prepare(out: &mut [u8], template: &[u8]) -> Result<usize> {
    res(unsafe {
        hooks_core::prepare(
            out.as_mut_ptr() as u32,
            out.len() as u32,
            template.as_ptr() as u32,
            template.len() as u32,
        )
    })
    .map(|v| v as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HookError;
    use crate::types::EMIT_DETAILS_MAX_LEN;

    #[test]
    fn smoke_not_implemented_on_host() {
        assert_eq!(etxn_burden(), Err(HookError::NotImplemented));
        let mut ed_out = [0u8; EMIT_DETAILS_MAX_LEN];
        assert_eq!(etxn_details(&mut ed_out), Err(HookError::NotImplemented));
        assert_eq!(etxn_fee_base(&[0u8; 4]), Err(HookError::NotImplemented));
        assert_eq!(etxn_reserve(1), Err(HookError::NotImplemented));
        assert_eq!(etxn_generation(), Err(HookError::NotImplemented));
        assert_eq!(etxn_nonce(), Err(HookError::NotImplemented));
        assert_eq!(emit(&[0u8; 4]), Err(HookError::NotImplemented));
        let mut out = [0u8; 8];
        assert_eq!(prepare(&mut out, &[0u8; 4]), Err(HookError::NotImplemented));
    }
}
