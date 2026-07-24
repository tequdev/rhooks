//! Information about the originating transaction (the transaction that
//! triggered this hook invocation).

use crate::error::{Result, res};
use crate::types::{HASH_LEN, Hash};

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
pub fn otxn_field(out: &mut [u8], field_id: u32) -> Result<usize> {
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
pub fn otxn_id(out: &mut [u8], flags: u32) -> Result<usize> {
    res(unsafe { hooks_core::otxn_id(out.as_mut_ptr() as u32, out.len() as u32, flags) })
        .map(|v| v as usize)
}

/// The ID (hash) of the originating transaction. `flags = 0` prefers the
/// emit-failure transaction ID where applicable; other flag values are
/// passed through verbatim (undocumented beyond that in the upstream Hook
/// API reference, so exposed as a plain `u32` rather than an invented enum).
#[inline(always)]
pub fn otxn_id_buf(flags: u32) -> Result<Hash> {
    let mut buf: Hash = [0u8; HASH_LEN];
    let _ = otxn_id(&mut buf, flags)?;
    Ok(buf)
}

/// The `TxType` of the originating transaction (see `hooks_core::tts`).
#[inline(always)]
pub fn otxn_type() -> u16 {
    unsafe { hooks_core::otxn_type() as u16 }
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
pub fn otxn_param(out: &mut [u8], name: &[u8]) -> Result<usize> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HookError;

    #[test]
    fn smoke_not_implemented_on_host() {
        assert_eq!(otxn_burden(), hooks_core::NOT_IMPLEMENTED as u64);
        assert_eq!(otxn_generation(), hooks_core::NOT_IMPLEMENTED as u32);
        assert_eq!(otxn_type(), hooks_core::NOT_IMPLEMENTED as u16);
        assert_eq!(otxn_slot(0), Err(HookError::NotImplemented));
        assert_eq!(otxn_id_buf(0), Err(HookError::NotImplemented));
        let mut buf = [0u8; 32];
        assert_eq!(otxn_id(&mut buf, 0), Err(HookError::NotImplemented));
        assert_eq!(otxn_field(&mut buf, 0), Err(HookError::NotImplemented));
        assert_eq!(otxn_field_u64(0), Err(HookError::NotImplemented));
        assert_eq!(otxn_param(&mut buf, b"x"), Err(HookError::NotImplemented));
    }
}
