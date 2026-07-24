//! Information about the executing hook itself: its account, hash, and
//! parameters.

use crate::error::{HookError, Result, res};
use crate::types::{ACC_ID_LEN, AccountId, HASH_LEN, Hash};

/// The AccountID this hook is installed on, written into `out`. Returns the
/// number of bytes written. [`hook_account_buf`] is the fixed-size
/// convenience twin.
#[inline(always)]
pub fn hook_account(out: &mut [u8]) -> Result<usize> {
    res(unsafe { hooks_core::hook_account(out.as_mut_ptr() as u32, out.len() as u32) })
        .map(|v| v as usize)
}

/// The AccountID this hook is installed on.
#[inline(always)]
pub fn hook_account_buf() -> Result<AccountId> {
    let mut buf = AccountId([0u8; ACC_ID_LEN]);
    let _ = hook_account(buf.as_mut())?;
    Ok(buf)
}

/// The hash of the hook definition at chain position `hook_no`, written into
/// `out`. Returns the number of bytes written. [`hook_hash_buf`] is the
/// fixed-size convenience twin.
#[inline(always)]
pub fn hook_hash(out: &mut [u8], hook_no: i32) -> Result<usize> {
    res(unsafe { hooks_core::hook_hash(out.as_mut_ptr() as u32, out.len() as u32, hook_no) })
        .map(|v| v as usize)
}

/// The hash of the hook definition at chain position `hook_no` on this
/// hook's account (negative indices address relative to the current hook,
/// per Hook API convention).
#[inline(always)]
pub fn hook_hash_buf(hook_no: i32) -> Result<Hash> {
    let mut buf = Hash([0u8; HASH_LEN]);
    let _ = hook_hash(buf.as_mut(), hook_no)?;
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

/// Read this hook's own parameter `name`, requiring it to be exactly `N`
/// bytes. A parameter longer than `N` already fails as
/// [`crate::error::HookError::TooSmall`] from the underlying host call
/// (`out`'s capacity is exactly `N`); a parameter shorter than `N` is caught
/// here and mapped to the same variant — see `state_exact` (`state.rs`) for
/// the identical pattern and rationale. No loop, no panic.
///
/// # Examples
///
/// ```
/// use hooks_lib::api::hook_ctx::hook_param_exact;
/// use hooks_lib::error::HookError;
///
/// assert_eq!(hook_param_exact::<4>(b"x"), Err(HookError::NotImplemented));
/// ```
#[inline(always)]
pub fn hook_param_exact<const N: usize>(name: &[u8]) -> Result<[u8; N]> {
    let mut out = [0u8; N];
    let written = hook_param(&mut out, name)?;
    if written == N {
        Ok(out)
    } else {
        Err(HookError::TooSmall)
    }
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
        assert_eq!(hook_account_buf(), Err(HookError::NotImplemented));
        assert_eq!(hook_hash_buf(0), Err(HookError::NotImplemented));
        let mut out = [0u8; 32];
        assert_eq!(hook_account(&mut out), Err(HookError::NotImplemented));
        assert_eq!(hook_hash(&mut out, 0), Err(HookError::NotImplemented));
        assert_eq!(hook_param(&mut out, b"x"), Err(HookError::NotImplemented));
        assert_eq!(
            hook_param_set(b"v", b"x", &[0u8; 32]),
            Err(HookError::NotImplemented)
        );
        assert_eq!(hook_param_exact::<4>(b"x"), Err(HookError::NotImplemented));
    }
}
