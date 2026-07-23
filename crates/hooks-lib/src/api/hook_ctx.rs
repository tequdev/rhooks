//! Information about the executing hook itself: its account, hash, and
//! parameters.

use crate::error::{Result, res};
use crate::types::{ACC_ID_LEN, AccountId, HASH_LEN, Hash};

/// The AccountID this hook is installed on.
#[inline(always)]
pub fn hook_account() -> Result<AccountId> {
    let mut buf: AccountId = [0u8; ACC_ID_LEN];
    res(unsafe { hooks_core::hook_account(buf.as_mut_ptr() as u32, buf.len() as u32) })?;
    Ok(buf)
}

/// The hash of the hook definition at chain position `hook_no` on this
/// hook's account (negative indices address relative to the current hook,
/// per Hook API convention).
#[inline(always)]
pub fn hook_hash(hook_no: i32) -> Result<Hash> {
    let mut buf: Hash = [0u8; HASH_LEN];
    res(unsafe { hooks_core::hook_hash(buf.as_mut_ptr() as u32, buf.len() as u32, hook_no) })?;
    Ok(buf)
}

/// Read this hook's own parameter `name` into `out`. Returns the number of
/// bytes written.
#[inline(always)]
pub fn hook_param(out: &mut [u8], name: &[u8]) -> Result<usize> {
    res(unsafe {
        hooks_core::hook_param(
            out.as_mut_ptr() as u32,
            out.len() as u32,
            name.as_ptr() as u32,
            name.len() as u32,
        )
    })
    .map(|v| v as usize)
}

/// Set a parameter named `name` to `value` on the hook identified by
/// `hook_hash`. Returns the number of bytes written.
#[inline(always)]
pub fn hook_param_set(value: &[u8], name: &[u8], hook_hash: &[u8]) -> Result<usize> {
    res(unsafe {
        hooks_core::hook_param_set(
            value.as_ptr() as u32,
            value.len() as u32,
            name.as_ptr() as u32,
            name.len() as u32,
            hook_hash.as_ptr() as u32,
            hook_hash.len() as u32,
        )
    })
    .map(|v| v as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HookError;

    #[test]
    fn smoke_not_implemented_on_host() {
        assert_eq!(hook_account(), Err(HookError::NotImplemented));
        assert_eq!(hook_hash(0), Err(HookError::NotImplemented));
        let mut out = [0u8; 32];
        assert_eq!(hook_param(&mut out, b"x"), Err(HookError::NotImplemented));
        assert_eq!(
            hook_param_set(b"v", b"x", &[0u8; 32]),
            Err(HookError::NotImplemented)
        );
    }
}
