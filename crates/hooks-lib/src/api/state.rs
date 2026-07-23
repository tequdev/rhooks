//! Persistent hook state: `state`, `state_set`, `state_foreign(_set)`.

use crate::error::{Result, res};

/// Read this hook's own state, decoded as an optional-defaulting `(ptr, len)`
/// pair — `None` becomes `(0, 0)`. Only ever used in the read direction,
/// never mixed with a write-direction call, so it does not blur the
/// pointer-direction discipline used elsewhere in this crate.
#[inline(always)]
fn opt_in(o: Option<&[u8]>) -> (u32, u32) {
    match o {
        Some(s) => (s.as_ptr() as u32, s.len() as u32),
        None => (0, 0),
    }
}

/// Read this hook's own state entry for `key` into `out`. Returns the number
/// of bytes written.
///
/// # Examples
///
/// ```
/// use hooks_lib::api::state::state;
/// use hooks_lib::error::HookError;
///
/// let mut out = [0u8; 32];
/// let key = [0u8; 32];
/// assert_eq!(state(&mut out, &key), Err(HookError::NotImplemented));
/// ```
#[inline(always)]
pub fn state(out: &mut [u8], key: &[u8]) -> Result<usize> {
    res(unsafe {
        hooks_core::state(
            out.as_mut_ptr() as u32,
            out.len() as u32,
            key.as_ptr() as u32,
            key.len() as u32,
        )
    })
    .map(|v| v as usize)
}

/// Write this hook's own state entry for `key`. Returns the number of bytes
/// written.
#[inline(always)]
pub fn state_set(data: &[u8], key: &[u8]) -> Result<usize> {
    res(unsafe {
        hooks_core::state_set(
            data.as_ptr() as u32,
            data.len() as u32,
            key.as_ptr() as u32,
            key.len() as u32,
        )
    })
    .map(|v| v as usize)
}

/// Read a state entry belonging to another account/namespace. `namespace`
/// and `account` default to "this hook's own" (`None` → the documented
/// zero-length Hook API sentinel) when omitted. Returns the number of bytes
/// written to `out`.
#[inline(always)]
pub fn state_foreign(
    out: &mut [u8],
    key: &[u8],
    namespace: Option<&[u8]>,
    account: Option<&[u8]>,
) -> Result<usize> {
    let (nptr, nlen) = opt_in(namespace);
    let (aptr, alen) = opt_in(account);
    res(unsafe {
        hooks_core::state_foreign(
            out.as_mut_ptr() as u32,
            out.len() as u32,
            key.as_ptr() as u32,
            key.len() as u32,
            nptr,
            nlen,
            aptr,
            alen,
        )
    })
    .map(|v| v as usize)
}

/// Write a state entry belonging to another namespace/account (a foreign
/// namespace on this hook's own account, or another account's namespace
/// depending on protocol rules). See [`state_foreign`] for the `Option`
/// convention. Returns the number of bytes written.
#[inline(always)]
pub fn state_foreign_set(
    data: &[u8],
    key: &[u8],
    namespace: Option<&[u8]>,
    account: Option<&[u8]>,
) -> Result<usize> {
    let (nptr, nlen) = opt_in(namespace);
    let (aptr, alen) = opt_in(account);
    res(unsafe {
        hooks_core::state_foreign_set(
            data.as_ptr() as u32,
            data.len() as u32,
            key.as_ptr() as u32,
            key.len() as u32,
            nptr,
            nlen,
            aptr,
            alen,
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
        let mut out = [0u8; 32];
        let key = [0u8; 32];
        assert_eq!(state(&mut out, &key), Err(HookError::NotImplemented));
        assert_eq!(state_set(&out, &key), Err(HookError::NotImplemented));
        assert_eq!(
            state_foreign(&mut out, &key, None, None),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            state_foreign_set(&out, &key, Some(&key), Some(&key)),
            Err(HookError::NotImplemented)
        );
    }
}
