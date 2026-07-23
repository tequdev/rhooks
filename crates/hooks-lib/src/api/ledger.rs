//! Ledger information: fee base, sequence, timestamps, hashes, nonces, and
//! keylet computation.
//!
//! `fee_base`, `ledger_seq`, and `ledger_last_time` are naturally unsigned
//! magnitudes despite the Hook API's `i64` wire type, so they are returned
//! as `u64` here (see [`crate::api::etxn`] for the same convention).

use crate::error::{Result, res};
use crate::types::{HASH_LEN, Hash, KEYLET_LEN, Keylet, NONCE_LEN, Nonce};

/// The reference transaction fee (in drops) for the current ledger.
#[inline(always)]
pub fn fee_base() -> Result<u64> {
    res(unsafe { hooks_core::fee_base() }).map(|v| v as u64)
}

/// The sequence number of the current ledger.
#[inline(always)]
pub fn ledger_seq() -> Result<u64> {
    res(unsafe { hooks_core::ledger_seq() }).map(|v| v as u64)
}

/// The close time of the previous ledger (seconds since the Ripple epoch).
#[inline(always)]
pub fn ledger_last_time() -> Result<u64> {
    res(unsafe { hooks_core::ledger_last_time() }).map(|v| v as u64)
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
        assert_eq!(fee_base(), Err(HookError::NotImplemented));
        assert_eq!(ledger_seq(), Err(HookError::NotImplemented));
        assert_eq!(ledger_last_time(), Err(HookError::NotImplemented));
        assert_eq!(ledger_last_hash(), Err(HookError::NotImplemented));
        assert_eq!(ledger_nonce(), Err(HookError::NotImplemented));
        assert_eq!(
            ledger_keylet(&[0u8; 34], &[0u8; 34]),
            Err(HookError::NotImplemented)
        );
    }
}
