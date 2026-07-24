//! Ledger information: fee base, sequence, timestamps, hashes, nonces, and
//! keylet computation.
//!
//! `fee_base`, `ledger_seq`, and `ledger_last_time` never return Hook API
//! error codes, so they are exposed as plain (non-`Result`) values, cast
//! from the `i64` wire type to their natural unsigned widths.

use crate::error::{Result, res};
use crate::types::{HASH_LEN, Hash, KEYLET_LEN, Keylet, NONCE_LEN, Nonce};

/// The reference transaction fee (in drops) for the current ledger.
#[inline(always)]
pub fn fee_base() -> u64 {
    unsafe { hooks_core::fee_base() as u64 }
}

/// The sequence number of the current ledger.
#[inline(always)]
pub fn ledger_seq() -> u32 {
    unsafe { hooks_core::ledger_seq() as u32 }
}

/// The close time of the previous ledger (seconds since the Ripple epoch).
#[inline(always)]
pub fn ledger_last_time() -> u64 {
    unsafe { hooks_core::ledger_last_time() as u64 }
}

/// The hash of the previous (parent) ledger, written into `out`. Returns the
/// number of bytes written. [`ledger_last_hash_buf`] is the fixed-size
/// convenience twin.
#[inline(always)]
pub fn ledger_last_hash(out: &mut [u8]) -> Result<usize> {
    res(unsafe { hooks_core::ledger_last_hash(out.as_mut_ptr() as u32, out.len() as u32) })
        .map(|v| v as usize)
}

/// The hash of the previous (parent) ledger.
#[inline(always)]
pub fn ledger_last_hash_buf() -> Result<Hash> {
    let mut buf: Hash = [0u8; HASH_LEN];
    let _ = ledger_last_hash(&mut buf)?;
    Ok(buf)
}

/// A ledger-derived nonce value, written into `out`. Returns the number of
/// bytes written. [`ledger_nonce_buf`] is the fixed-size convenience twin.
#[inline(always)]
pub fn ledger_nonce(out: &mut [u8]) -> Result<usize> {
    res(unsafe { hooks_core::ledger_nonce(out.as_mut_ptr() as u32, out.len() as u32) })
        .map(|v| v as usize)
}

/// A ledger-derived nonce value (distinct from [`crate::api::etxn::etxn_nonce`],
/// which is per-emission).
#[inline(always)]
pub fn ledger_nonce_buf() -> Result<Nonce> {
    let mut buf: Nonce = [0u8; NONCE_LEN];
    let _ = ledger_nonce(&mut buf)?;
    Ok(buf)
}

/// Compute a Keylet from a low/high bound pair, written into `out`. Returns
/// the number of bytes written. [`ledger_keylet_buf`] is the fixed-size
/// convenience twin.
#[inline(always)]
pub fn ledger_keylet(out: &mut [u8], low: &[u8], high: &[u8]) -> Result<usize> {
    res(unsafe {
        hooks_core::ledger_keylet(
            out.as_mut_ptr() as u32,
            out.len() as u32,
            low.as_ptr() as u32,
            low.len() as u32,
            high.as_ptr() as u32,
            high.len() as u32,
        )
    })
    .map(|v| v as usize)
}

/// Compute a Keylet from a low/high bound pair (as used by range-style
/// ledger entries).
#[inline(always)]
pub fn ledger_keylet_buf(low: &[u8], high: &[u8]) -> Result<Keylet> {
    let mut buf: Keylet = [0u8; KEYLET_LEN];
    let _ = ledger_keylet(&mut buf, low, high)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HookError;

    #[test]
    fn smoke_not_implemented_on_host() {
        assert_eq!(fee_base(), hooks_core::NOT_IMPLEMENTED as u64);
        assert_eq!(ledger_seq(), hooks_core::NOT_IMPLEMENTED as u32);
        assert_eq!(ledger_last_time(), hooks_core::NOT_IMPLEMENTED as u64);
        assert_eq!(ledger_last_hash_buf(), Err(HookError::NotImplemented));
        assert_eq!(ledger_nonce_buf(), Err(HookError::NotImplemented));
        assert_eq!(
            ledger_keylet_buf(&[0u8; 34], &[0u8; 34]),
            Err(HookError::NotImplemented)
        );
        let mut out = [0u8; 34];
        assert_eq!(ledger_last_hash(&mut out), Err(HookError::NotImplemented));
        assert_eq!(ledger_nonce(&mut out), Err(HookError::NotImplemented));
        assert_eq!(
            ledger_keylet(&mut out, &[0u8; 34], &[0u8; 34]),
            Err(HookError::NotImplemented)
        );
    }
}
