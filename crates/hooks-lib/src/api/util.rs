//! Address conversion, hashing, signature verification, and keylet
//! computation utilities.

use crate::error::{Result, res};
use crate::types::{ACC_ID_LEN, AccountId, HASH_LEN, Hash, KEYLET_LEN, Keylet};

/// Convert an AccountID (`accid`) to its base58 r-address text form,
/// written into `out`. Variable-length text output, so this stays on the
/// caller-buffer `Result<usize>` convention rather than a fixed-size
/// convenience wrapper.
#[inline(always)]
pub fn util_raddr(out: &mut [u8], accid: &[u8]) -> Result<usize> {
    res(unsafe {
        hooks_core::util_raddr(
            out.as_mut_ptr() as u32,
            out.len() as u32,
            accid.as_ptr() as u32,
            accid.len() as u32,
        )
    })
    .map(|v| v as usize)
}

/// Convert a base58 r-address (`r_address`) to its AccountID form.
#[inline(always)]
pub fn util_accid(r_address: &[u8]) -> Result<AccountId> {
    let mut buf: AccountId = [0u8; ACC_ID_LEN];
    res(unsafe {
        hooks_core::util_accid(
            buf.as_mut_ptr() as u32,
            buf.len() as u32,
            r_address.as_ptr() as u32,
            r_address.len() as u32,
        )
    })?;
    Ok(buf)
}

/// Verify that `signature` over `data` was produced by the key `public_key`.
#[inline(always)]
pub fn util_verify(data: &[u8], signature: &[u8], public_key: &[u8]) -> Result<bool> {
    res(unsafe {
        hooks_core::util_verify(
            data.as_ptr() as u32,
            data.len() as u32,
            signature.as_ptr() as u32,
            signature.len() as u32,
            public_key.as_ptr() as u32,
            public_key.len() as u32,
        )
    })
    .map(|v| v != 0)
}

/// SHA-512-Half of `data`.
#[inline(always)]
pub fn util_sha512h(data: &[u8]) -> Result<Hash> {
    let mut buf: Hash = [0u8; HASH_LEN];
    res(unsafe {
        hooks_core::util_sha512h(
            buf.as_mut_ptr() as u32,
            buf.len() as u32,
            data.as_ptr() as u32,
            data.len() as u32,
        )
    })?;
    Ok(buf)
}

/// Compute a Keylet of `keylet_type` from up to six `u32` components
/// (`a`..`f`; unused components are `0` per the Hook API convention for the
/// given keylet type — see `hooks_core::KEYLET_*`).
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub fn util_keylet(
    keylet_type: u32,
    a: u32,
    b: u32,
    c: u32,
    d: u32,
    e: u32,
    f: u32,
) -> Result<Keylet> {
    let mut buf: Keylet = [0u8; KEYLET_LEN];
    res(unsafe {
        hooks_core::util_keylet(
            buf.as_mut_ptr() as u32,
            buf.len() as u32,
            keylet_type,
            a,
            b,
            c,
            d,
            e,
            f,
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
        let mut out = [0u8; 64];
        assert_eq!(
            util_raddr(&mut out, &[0u8; 20]),
            Err(HookError::NotImplemented)
        );
        assert_eq!(util_accid(b"raddress"), Err(HookError::NotImplemented));
        assert_eq!(
            util_verify(b"data", b"sig", b"pubkey"),
            Err(HookError::NotImplemented)
        );
        assert_eq!(util_sha512h(b"data"), Err(HookError::NotImplemented));
        assert_eq!(
            util_keylet(1, 0, 0, 0, 0, 0, 0),
            Err(HookError::NotImplemented)
        );
    }
}
