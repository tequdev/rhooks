//! Information about the executing hook itself: its account, hash, and
//! parameters.

use crate::convert::FixedRead;
use crate::error::{Result, res};
use crate::types::{AccountId, Hash};

/// The AccountID this hook is installed on, written into `out`. Returns the
/// number of bytes written. [`hook_account_buf`] is the fixed-size
/// convenience twin.
#[inline(always)]
pub fn hook_account<B: AsMut<[u8]> + ?Sized>(out: &mut B) -> Result<usize> {
    let out = out.as_mut();
    #[cfg(all(feature = "testenv", not(target_arch = "wasm32")))]
    {
        if let Some(result) = hooks_core::backend::with_backend(|b| b.hook_account()) {
            return crate::testenv_bridge::write_bytes(out, result);
        }
    }
    res(unsafe { hooks_core::hook_account(out.as_mut_ptr() as u32, out.len() as u32) })
        .map(|v| v as usize)
}

/// The AccountID this hook is installed on.
#[inline(always)]
pub fn hook_account_buf() -> Result<AccountId> {
    let mut buf = AccountId::default();
    let _ = hook_account(buf.as_mut())?;
    Ok(buf)
}

/// The hash of the hook definition at chain position `hook_no`, written into
/// `out`. Returns the number of bytes written. [`hook_hash_buf`] is the
/// fixed-size convenience twin.
#[inline(always)]
pub fn hook_hash<B: AsMut<[u8]> + ?Sized>(out: &mut B, hook_no: i32) -> Result<usize> {
    let out = out.as_mut();
    #[cfg(all(feature = "testenv", not(target_arch = "wasm32")))]
    {
        if let Some(result) = hooks_core::backend::with_backend(|b| b.hook_hash(hook_no)) {
            return crate::testenv_bridge::write_bytes(out, result);
        }
    }
    res(unsafe { hooks_core::hook_hash(out.as_mut_ptr() as u32, out.len() as u32, hook_no) })
        .map(|v| v as usize)
}

/// The hash of the hook definition at chain position `hook_no` on this
/// hook's account (negative indices address relative to the current hook,
/// per Hook API convention).
#[inline(always)]
pub fn hook_hash_buf(hook_no: i32) -> Result<Hash> {
    let mut buf = Hash::default();
    let _ = hook_hash(buf.as_mut(), hook_no)?;
    Ok(buf)
}

/// Read this hook's own parameter `name` into `out`. Returns the number of
/// bytes written.
#[inline(always)]
pub fn hook_param<B: AsMut<[u8]> + ?Sized>(out: &mut B, name: &[u8]) -> Result<usize> {
    let out = out.as_mut();
    #[cfg(all(feature = "testenv", not(target_arch = "wasm32")))]
    {
        if let Some(result) = hooks_core::backend::with_backend(|b| b.hook_param(name)) {
            return crate::testenv_bridge::write_bytes(out, result);
        }
    }
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

/// Read this hook's own parameter `name`, requiring it to be exactly `T`'s
/// length — any [`crate::convert::FixedRead`] type. A parameter longer than
/// that already fails as [`crate::error::HookError::TooSmall`] from the
/// underlying host call; a parameter shorter is caught by `T::read_exact`
/// itself and mapped to the same variant — see `state_exact` (`state.rs`)
/// for the identical pattern and rationale. No loop, no panic.
///
/// `T` is inferred from context, not a turbofish — see
/// [`crate::api::otxn::otxn_field_exact`]'s doc comment for the full
/// story.
///
/// # Examples
///
/// ```
/// use hooks_lib::api::hook_ctx::hook_param_exact;
/// use hooks_lib::error::{HookError, Result};
///
/// let value: Result<[u8; 4]> = hook_param_exact(b"x");
/// assert_eq!(value, Err(HookError::NotImplemented));
/// ```
#[inline(always)]
pub fn hook_param_exact<T: FixedRead>(name: &[u8]) -> Result<T> {
    T::read_exact(|buf| hook_param(buf, name))
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
        assert_eq!(
            hook_param_exact::<[u8; 4]>(b"x"),
            Err(HookError::NotImplemented)
        );
    }
}
