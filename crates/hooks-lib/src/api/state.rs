//! Persistent hook state: `state`, `state_set`, `state_foreign(_set)`.
//!
//! Two distinct integer conventions coexist in this module, deliberately:
//! - [`state_u64`] (and `state_foreign_u64`) use the host's "as-int64" mode
//!   (`write_ptr = 0, write_len = 0`) — the host packs the entry's raw bytes
//!   **big-endian** into the returned `i64` (see DESIGN.md §5.2). There is
//!   no write-side as-int64 mode ([`crate::error::HookError`]'s module doc
//!   notes `state_set` "carr[ies] no write buffer"), so [`state_update_u64`]
//!   writes back with `to_be_bytes` to round-trip through that same
//!   big-endian convention.
//! - [`state_u32`], [`state_i64`], [`state_xfl`] (and their `state_set_*`/
//!   `state_update_*` twins) instead read/write a plain fixed-size buffer
//!   via [`state_exact`] and decode/encode it **little-endian** — the
//!   convention the `state-counter` example already uses by hand
//!   (`u64::from_le_bytes`/`to_le_bytes` around a plain `state`/`state_set`
//!   round-trip). These do not use the host's as-int64 mode at all.
//!
//! Every helper above is a standalone function keyed by a raw `&[u8]` — for
//! a typed layer where the key itself is a compile-time-checked enum
//! variant and the value can be any [`crate::convert::ToBytes`]/
//! [`crate::convert::FromBytes`] type (including every `hooks_lib::types`
//! newtype), see [`mod@crate::state`]'s `state_get`/`state_set_typed`/
//! `state_update_typed` and the [`state_keys!`](crate::state_keys) macro,
//! built on top of this module's [`state`]/[`state_set`]/[`state_exact`].
//!
//! ## Why every `key` parameter here is `&(impl AsRef<[u8]> + ?Sized)`, not `&[u8]`
//!
//! Every key-shaped parameter in this module (`state`'s `key`, `state_set`'s
//! `key`, `state_foreign`'s `key`, ...) is generic instead of a bare `&[u8]`
//! specifically so a [`crate::types::StateKey`] (or any other
//! `hooks_lib::types` newtype a hook chooses to key state with) can be
//! passed straight through as `&STATE_KEY`, with no `.as_ref()` at the call
//! site. A bare `&[u8]` parameter can't do this: `StateKey` only
//! implements [`core::ops::Deref`] with `[u8; 32]` as its target (not
//! `[u8]` — see `crate::types`' module doc comment for why), and Rust does
//! not chain that one `Deref` hop with the further built-in array-to-slice
//! unsized coercion at a single call site, so `&STATE_KEY` alone never
//! reaches a `&[u8]` parameter. Bounding the parameter by
//! [`AsRef<[u8]>`](AsRef) instead sidesteps the coercion question entirely:
//! `StateKey` (and every other `crate::types` newtype) already implements
//! `AsRef<[u8]>` directly, no coercion needed. This is zero-cost —
//! `#[inline(always)]` plus one generic parameter monomorphized per call
//! site compiles to the exact same code as the old concrete `&[u8]`
//! parameter did (verified: `mise run build-examples`'s per-example wasm
//! size and worst-case instruction count are unchanged by this).
//!
//! `namespace`/`account` (`state_foreign`'s `Option<&[u8]>` pair)
//! deliberately stay a concrete `Option<&[u8]>`, not `Option<K: AsRef<[u8]>>`:
//! a generic `Option<K>` parameter cannot also accept a bare `None`
//! literal — with `K` unconstrained by anything else in the call, `None`
//! never pins down a single `K`, so the call becomes ambiguous and fails to
//! compile (verified directly against rustc, not just reasoned about).
//! Passing a newtype through one of these still needs `Some(value.as_ref())`.

use crate::error::{HookError, Result, res};
use crate::xfl::XFL;

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
pub fn state<K: AsRef<[u8]> + ?Sized>(out: &mut [u8], key: &K) -> Result<usize> {
    let key = key.as_ref();
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

/// Read this hook's own state entry for `key` as a big-endian `u64`
/// ("as-int64" mode: the host is passed `write_ptr = 0, write_len = 0` and
/// returns the entry's bytes packed big-endian instead of a length).
///
/// Only valid for entries of at most 8 bytes whose top bit is clear —
/// anything else fails with [`crate::error::HookError::TooBig`] (xahaud's
/// `data_as_int64`, `applyHook.cpp`). Note the interpretation is
/// **big-endian**: an 8-byte little-endian counter read this way comes back
/// byte-swapped.
#[inline(always)]
pub fn state_u64<K: AsRef<[u8]> + ?Sized>(key: &K) -> Result<u64> {
    let key = key.as_ref();
    res(unsafe { hooks_core::state(0, 0, key.as_ptr() as u32, key.len() as u32) }).map(|v| v as u64)
}

/// Read this hook's own state entry for `key`, requiring it to be exactly
/// `N` bytes.
///
/// An entry longer than `N` already fails as
/// [`HookError::TooSmall`](crate::error::HookError::TooSmall) from the
/// underlying host call (`out`'s capacity is exactly `N`); an entry shorter
/// than `N` is caught here (the host call succeeds but writes fewer than `N`
/// bytes) and mapped to the same [`HookError::TooSmall`] variant, since both
/// cases are "the entry does not have the exact expected size". No loop, no
/// panic: a fixed `[0u8; N]` buffer plus one length comparison.
///
/// # Examples
///
/// ```
/// use hooks_lib::api::state::state_exact;
/// use hooks_lib::error::HookError;
///
/// let key = [0u8; 32];
/// assert_eq!(state_exact::<8>(&key), Err(HookError::NotImplemented));
/// ```
#[inline(always)]
pub fn state_exact<const N: usize>(key: &[u8]) -> Result<[u8; N]> {
    let mut out = [0u8; N];
    let written = state(&mut out, key)?;
    if written == N {
        Ok(out)
    } else {
        Err(HookError::TooSmall)
    }
}

/// Read this hook's own state entry for `key` as a little-endian `u32` (via
/// [`state_exact`] — see the module doc comment for how this differs from
/// [`state_u64`]'s host-side as-int64 mode).
///
/// # Examples
///
/// ```
/// use hooks_lib::api::state::state_u32;
/// use hooks_lib::error::HookError;
///
/// let key = [0u8; 32];
/// assert_eq!(state_u32(&key), Err(HookError::NotImplemented));
/// ```
#[inline(always)]
pub fn state_u32<K: AsRef<[u8]> + ?Sized>(key: &K) -> Result<u32> {
    state_exact::<4>(key.as_ref()).map(u32::from_le_bytes)
}

/// Write this hook's own state entry for `key` as a little-endian `u32`.
/// Returns the number of bytes written.
#[inline(always)]
pub fn state_set_u32<K: AsRef<[u8]> + ?Sized>(value: u32, key: &K) -> Result<usize> {
    state_set(&value.to_le_bytes(), key)
}

/// Read this hook's own state entry for `key` as a little-endian `i64` (via
/// [`state_exact`]; see the module doc comment).
#[inline(always)]
pub fn state_i64<K: AsRef<[u8]> + ?Sized>(key: &K) -> Result<i64> {
    state_exact::<8>(key.as_ref()).map(i64::from_le_bytes)
}

/// Write this hook's own state entry for `key` as a little-endian `i64`.
/// Returns the number of bytes written.
#[inline(always)]
pub fn state_set_i64<K: AsRef<[u8]> + ?Sized>(value: i64, key: &K) -> Result<usize> {
    state_set(&value.to_le_bytes(), key)
}

/// Read this hook's own state entry for `key` as an [`XFL`], stored as its
/// raw bit pattern (`i64`) in little-endian bytes (via [`state_exact`]; see
/// the module doc comment).
#[inline(always)]
pub fn state_xfl<K: AsRef<[u8]> + ?Sized>(key: &K) -> Result<XFL> {
    state_exact::<8>(key.as_ref())
        .map(i64::from_le_bytes)
        .map(XFL::from_raw_bits)
}

/// Write this hook's own state entry for `key` as an [`XFL`]'s raw bit
/// pattern (`i64`), little-endian. Returns the number of bytes written.
#[inline(always)]
pub fn state_set_xfl<K: AsRef<[u8]> + ?Sized>(value: XFL, key: &K) -> Result<usize> {
    state_set(&value.raw_bits().to_le_bytes(), key)
}

/// Collapse a state-read [`Result`] into "value present" (`Ok(Some(v))`),
/// "no entry yet" (`Ok(None)`, from
/// [`HookError::DoesntExist`](crate::error::HookError::DoesntExist)), or "a
/// real error" (`Err(e)`, everything else — including
/// [`HookError::NotImplemented`](crate::error::HookError::NotImplemented) on
/// host builds). Pure function, no host call — kept separate from the
/// `state_update_*` functions so it has a standalone unit test.
#[inline(always)]
fn absent_as_none<T>(result: Result<T>) -> Result<Option<T>> {
    match result {
        Ok(v) => Ok(Some(v)),
        Err(HookError::DoesntExist) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Read-modify-write this hook's own state entry for `key` as a `u64`
/// (host as-int64 big-endian convention, matching [`state_u64`]): `f` is
/// called with `None` if the key does not yet exist, or `Some` of the
/// current value otherwise; its return value is written back and also
/// returned as `Ok`. A real read error (anything but "doesn't exist")
/// short-circuits before `f` is called or anything is written.
#[inline(always)]
pub fn state_update_u64<K: AsRef<[u8]> + ?Sized>(
    key: &K,
    f: impl FnOnce(Option<u64>) -> u64,
) -> Result<u64> {
    let current = absent_as_none(state_u64(key))?;
    let next = f(current);
    let _ = state_set(&next.to_be_bytes(), key)?;
    Ok(next)
}

/// Read-modify-write this hook's own state entry for `key` as a `u32`
/// (little-endian convention, matching [`state_u32`]). See
/// [`state_update_u64`] for the `Option`/error-propagation semantics.
#[inline(always)]
pub fn state_update_u32<K: AsRef<[u8]> + ?Sized>(
    key: &K,
    f: impl FnOnce(Option<u32>) -> u32,
) -> Result<u32> {
    let current = absent_as_none(state_u32(key))?;
    let next = f(current);
    let _ = state_set_u32(next, key)?;
    Ok(next)
}

/// Read-modify-write this hook's own state entry for `key` as an `i64`
/// (little-endian convention, matching [`state_i64`]). See
/// [`state_update_u64`] for the `Option`/error-propagation semantics.
#[inline(always)]
pub fn state_update_i64<K: AsRef<[u8]> + ?Sized>(
    key: &K,
    f: impl FnOnce(Option<i64>) -> i64,
) -> Result<i64> {
    let current = absent_as_none(state_i64(key))?;
    let next = f(current);
    let _ = state_set_i64(next, key)?;
    Ok(next)
}

/// Read-modify-write this hook's own state entry for `key` as an [`XFL`]
/// (little-endian raw-bits convention, matching [`state_xfl`]). See
/// [`state_update_u64`] for the `Option`/error-propagation semantics.
#[inline(always)]
pub fn state_update_xfl<K: AsRef<[u8]> + ?Sized>(
    key: &K,
    f: impl FnOnce(Option<XFL>) -> XFL,
) -> Result<XFL> {
    let current = absent_as_none(state_xfl(key))?;
    let next = f(current);
    let _ = state_set_xfl(next, key)?;
    Ok(next)
}

/// Write this hook's own state entry for `key`. Returns the number of bytes
/// written.
#[inline(always)]
pub fn state_set<K: AsRef<[u8]> + ?Sized>(data: &[u8], key: &K) -> Result<usize> {
    let key = key.as_ref();
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
pub fn state_foreign<K: AsRef<[u8]> + ?Sized>(
    out: &mut [u8],
    key: &K,
    namespace: Option<&[u8]>,
    account: Option<&[u8]>,
) -> Result<usize> {
    let key = key.as_ref();
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

/// Read a foreign state entry as a big-endian `u64` ("as-int64" mode; see
/// [`state_u64`] for the size/top-bit rules and endianness caveat).
/// `namespace`/`account` follow [`state_foreign`]'s `Option` convention.
#[inline(always)]
pub fn state_foreign_u64<K: AsRef<[u8]> + ?Sized>(
    key: &K,
    namespace: Option<&[u8]>,
    account: Option<&[u8]>,
) -> Result<u64> {
    let key = key.as_ref();
    let (nptr, nlen) = opt_in(namespace);
    let (aptr, alen) = opt_in(account);
    res(unsafe {
        hooks_core::state_foreign(
            0,
            0,
            key.as_ptr() as u32,
            key.len() as u32,
            nptr,
            nlen,
            aptr,
            alen,
        )
    })
    .map(|v| v as u64)
}

/// Write a state entry belonging to another namespace/account (a foreign
/// namespace on this hook's own account, or another account's namespace
/// depending on protocol rules). See [`state_foreign`] for the `Option`
/// convention. Returns the number of bytes written.
#[inline(always)]
pub fn state_foreign_set<K: AsRef<[u8]> + ?Sized>(
    data: &[u8],
    key: &K,
    namespace: Option<&[u8]>,
    account: Option<&[u8]>,
) -> Result<usize> {
    let key = key.as_ref();
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
        assert_eq!(state_u64(&key), Err(HookError::NotImplemented));
        assert_eq!(
            state_foreign_u64(&key, None, None),
            Err(HookError::NotImplemented)
        );
        assert_eq!(state_set(&out, &key), Err(HookError::NotImplemented));
        assert_eq!(
            state_foreign(&mut out, &key, None, None),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            state_foreign_set(&out, &key, Some(&key), Some(&key)),
            Err(HookError::NotImplemented)
        );
        assert_eq!(state_exact::<8>(&key), Err(HookError::NotImplemented));
        assert_eq!(state_u32(&key), Err(HookError::NotImplemented));
        assert_eq!(state_set_u32(1, &key), Err(HookError::NotImplemented));
        assert_eq!(state_i64(&key), Err(HookError::NotImplemented));
        assert_eq!(state_set_i64(1, &key), Err(HookError::NotImplemented));
        assert!(matches!(state_xfl(&key), Err(HookError::NotImplemented)));
        assert_eq!(
            state_set_xfl(XFL::one(), &key),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            state_update_u64(&key, |cur| cur.unwrap_or(0) + 1),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            state_update_u32(&key, |cur| cur.unwrap_or(0) + 1),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            state_update_i64(&key, |cur| cur.unwrap_or(0) + 1),
            Err(HookError::NotImplemented)
        );
        assert!(matches!(
            state_update_xfl(&key, |cur| cur.unwrap_or(XFL::one())),
            Err(HookError::NotImplemented)
        ));
    }

    #[test]
    fn exact_rejects_short_write() {
        // Pure-logic check on the length comparison in `state_exact`,
        // independent of the host call: a `state()` that reports fewer
        // bytes written than the requested `N` must be `TooSmall`, not a
        // silently zero-padded array. `state_exact` itself always goes
        // through the host stub on this target (which returns
        // `NotImplemented` before any length check happens), so this test
        // exercises the same comparison the function performs, standalone.
        fn exact_from_written<const N: usize>(written: usize) -> Result<[u8; N]> {
            let out = [0u8; N];
            if written == N {
                Ok(out)
            } else {
                Err(HookError::TooSmall)
            }
        }
        assert_eq!(exact_from_written::<8>(8), Ok([0u8; 8]));
        assert_eq!(exact_from_written::<8>(4), Err(HookError::TooSmall));
    }

    #[test]
    fn absent_as_none_distinguishes_doesnt_exist_from_real_errors() {
        assert_eq!(absent_as_none(Ok(7u64)), Ok(Some(7u64)));
        assert_eq!(absent_as_none::<u64>(Err(HookError::DoesntExist)), Ok(None));
        assert_eq!(
            absent_as_none::<u64>(Err(HookError::NotImplemented)),
            Err(HookError::NotImplemented)
        );
        assert_eq!(
            absent_as_none::<u64>(Err(HookError::TooBig)),
            Err(HookError::TooBig)
        );
    }
}
