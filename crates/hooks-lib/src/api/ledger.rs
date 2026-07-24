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

/// The hash of the previous (parent) ledger.
#[inline(always)]
pub fn ledger_last_hash() -> Result<Hash> {
    let mut buf: Hash = [0u8; HASH_LEN];
    res(unsafe { hooks_core::ledger_last_hash(buf.as_mut_ptr() as u32, buf.len() as u32) })?;
    Ok(buf)
}

/// A ledger-derived nonce value (distinct from [`crate::api::etxn::etxn_nonce`],
/// which is per-emission).
#[inline(always)]
pub fn ledger_nonce() -> Result<Nonce> {
    let mut buf: Nonce = [0u8; NONCE_LEN];
    res(unsafe { hooks_core::ledger_nonce(buf.as_mut_ptr() as u32, buf.len() as u32) })?;
    Ok(buf)
}

/// Compute a Keylet from a low/high bound pair (as used by range-style
/// ledger entries).
#[inline(always)]
pub fn ledger_keylet(low: &[u8], high: &[u8]) -> Result<Keylet> {
    let mut buf: Keylet = [0u8; KEYLET_LEN];
    res(unsafe {
        hooks_core::ledger_keylet(
            buf.as_mut_ptr() as u32,
            buf.len() as u32,
            low.as_ptr() as u32,
            low.len() as u32,
            high.as_ptr() as u32,
            high.len() as u32,
        )
    })?;
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
        assert_eq!(ledger_last_hash(), Err(HookError::NotImplemented));
        assert_eq!(ledger_nonce(), Err(HookError::NotImplemented));
        assert_eq!(
            ledger_keylet(&[0u8; 34], &[0u8; 34]),
            Err(HookError::NotImplemented)
        );
    }
}
