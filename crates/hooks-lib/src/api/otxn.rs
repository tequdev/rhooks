//! Information about the originating transaction (the transaction that
//! triggered this hook invocation).

use crate::convert::FixedRead;
use crate::error::{Result, res};
use crate::tx_type::TxType;
use crate::types::Hash;

/// Burden of the originating transaction: `1` for a normal transaction, or
/// the `sfEmitBurden` value for an emitted transaction. A natural (unsigned)
/// magnitude despite the Hook API's `i64` wire type.
#[inline(always)]
pub fn otxn_burden() -> u64 {
    unsafe { hooks_core::otxn_burden() as u64 }
}

/// Read a field from the originating transaction into `out`. Returns the
/// number of bytes written.
#[inline(always)]
pub fn otxn_field<B: AsMut<[u8]> + ?Sized>(out: &mut B, field_id: u32) -> Result<usize> {
    let out = out.as_mut();
    res(unsafe { hooks_core::otxn_field(out.as_mut_ptr() as u32, out.len() as u32, field_id) })
        .map(|v| v as usize)
}

/// Read a field from the originating transaction as a big-endian `u64`
/// ("as-int64" mode: `write_ptr = 0, write_len = 0`; only for fields of at
/// most 8 bytes with the top bit clear, else
/// [`crate::error::HookError::TooBig`] — see `state_u64` for details).
#[inline(always)]
pub fn otxn_field_u64(field_id: u32) -> Result<u64> {
    res(unsafe { hooks_core::otxn_field(0, 0, field_id) }).map(|v| v as u64)
}

/// Read field `field_id` from the originating transaction, requiring it to
/// be exactly `T`'s length — any [`crate::convert::FixedRead`] type, most
/// commonly a `hooks_lib::types` newtype or a raw `[u8; N]`. A field longer
/// than that already fails as [`crate::error::HookError::TooSmall`] from the
/// underlying host call (the buffer `T::read_exact` allocates has exactly
/// that capacity); a field shorter is caught by `T::read_exact` itself and
/// mapped to the same variant — see `state_exact` (`state.rs`) for the
/// identical pattern and rationale. No loop, no panic.
///
/// `T` is inferred from context (a `let` binding's type annotation, a
/// function's declared return type, ...), not a turbofish — e.g.
/// `let sender: AccountId = otxn_field_exact(sfAccount)?;`. A call site
/// with no way to infer `T` is a compile error; annotate it (or use
/// `otxn_field_exact::<AccountId>(field_id)`/`::<[u8; 20]>(field_id)`
/// explicitly).
///
/// # Examples
///
/// ```
/// use hooks_lib::api::otxn::otxn_field_exact;
/// use hooks_lib::error::{HookError, Result};
///
/// let sender: Result<[u8; 20]> = otxn_field_exact(0);
/// assert_eq!(sender, Err(HookError::NotImplemented));
/// ```
#[inline(always)]
pub fn otxn_field_exact<T: FixedRead>(field_id: u32) -> Result<T> {
    T::read_exact(|buf| otxn_field(buf, field_id))
}

/// Generation of the originating transaction: `0` for a normal transaction,
/// or the `sfEmitGeneration` value for an emitted transaction.
#[inline(always)]
pub fn otxn_generation() -> u32 {
    unsafe { hooks_core::otxn_generation() as u32 }
}

/// The ID (hash) of the originating transaction, written into `out`.
/// Returns the number of bytes written. [`otxn_id_buf`] is the fixed-size
/// convenience twin.
#[inline(always)]
pub fn otxn_id<B: AsMut<[u8]> + ?Sized>(out: &mut B, flags: u32) -> Result<usize> {
    let out = out.as_mut();
    res(unsafe { hooks_core::otxn_id(out.as_mut_ptr() as u32, out.len() as u32, flags) })
        .map(|v| v as usize)
}

/// The ID (hash) of the originating transaction. `flags = 0` prefers the
/// emit-failure transaction ID where applicable; other flag values are
/// passed through verbatim (undocumented beyond that in the upstream Hook
/// API reference, so exposed as a plain `u32` rather than an invented enum).
#[inline(always)]
pub fn otxn_id_buf(flags: u32) -> Result<Hash> {
    let mut buf = Hash::default();
    let _ = otxn_id(buf.as_mut(), flags)?;
    Ok(buf)
}

/// The [`TxType`] of the originating transaction.
#[inline(always)]
pub fn otxn_type() -> TxType {
    TxType::from(unsafe { hooks_core::otxn_type() as u16 })
}

/// Load the originating transaction into a slot. `slot_into = 0` auto-assigns
/// a slot. Returns the assigned slot number.
#[inline(always)]
pub fn otxn_slot(slot_into: u32) -> Result<u32> {
    res(unsafe { hooks_core::otxn_slot(slot_into) }).map(|v| v as u32)
}

/// Read a Hook parameter attached to the originating transaction into `out`.
/// Returns the number of bytes written.
#[inline(always)]
pub fn otxn_param<B: AsMut<[u8]> + ?Sized>(out: &mut B, name: &[u8]) -> Result<usize> {
    let out = out.as_mut();
    res(unsafe {
        hooks_core::otxn_param(
            out.as_mut_ptr() as u32,
            out.len() as u32,
            name.as_ptr() as u32,
            name.len() as u32,
        )
    })
    .map(|v| v as usize)
}

/// Read a Hook parameter attached to the originating transaction, requiring
/// it to be exactly `T`'s length — any [`crate::convert::FixedRead`] type,
/// most commonly a `hooks_lib::types` newtype, a raw `[u8; N]`, or a
/// [`crate::HookData`]-derived struct. A parameter longer than that already
/// fails as [`crate::error::HookError::TooSmall`] from the underlying host
/// call; a parameter shorter is caught by `T::read_exact` itself and mapped
/// to the same variant — see [`otxn_field_exact`]/`state_exact` (`state.rs`)
/// for the identical pattern and rationale. No loop, no panic.
///
/// `T` is inferred from context, not a turbofish — see
/// [`otxn_field_exact`]'s doc comment for the full story.
///
/// # Examples
///
/// ```
/// use hooks_lib::api::otxn::otxn_param_exact;
/// use hooks_lib::error::{HookError, Result};
///
/// let value: Result<[u8; 4]> = otxn_param_exact(b"CFG");
/// assert_eq!(value, Err(HookError::NotImplemented));
/// ```
#[inline(always)]
pub fn otxn_param_exact<T: FixedRead>(name: &[u8]) -> Result<T> {
    T::read_exact(|buf| otxn_param(buf, name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HookError;

    #[test]
    fn smoke_not_implemented_on_host() {
        assert_eq!(otxn_burden(), hooks_core::NOT_IMPLEMENTED as u64);
        assert_eq!(otxn_generation(), hooks_core::NOT_IMPLEMENTED as u32);
        // The host stub's `NOT_IMPLEMENTED` (a negative i64) doesn't match
        // any real `tt*` code once truncated to `u16`, so this decodes to
        // `TxType::Unknown` rather than a specific known variant.
        assert_eq!(
            otxn_type(),
            TxType::Unknown(hooks_core::NOT_IMPLEMENTED as u16)
        );
        assert_eq!(otxn_slot(0), Err(HookError::NotImplemented));
        assert_eq!(otxn_id_buf(0), Err(HookError::NotImplemented));
        let mut buf = [0u8; 32];
        assert_eq!(otxn_id(&mut buf, 0), Err(HookError::NotImplemented));
        assert_eq!(otxn_field(&mut buf, 0), Err(HookError::NotImplemented));
        assert_eq!(otxn_field_u64(0), Err(HookError::NotImplemented));
        assert_eq!(
            otxn_field_exact::<[u8; 20]>(0),
            Err(HookError::NotImplemented)
        );
        assert_eq!(otxn_param(&mut buf, b"x"), Err(HookError::NotImplemented));
        assert_eq!(
            otxn_param_exact::<[u8; 4]>(b"x"),
            Err(HookError::NotImplemented)
        );
    }
}
