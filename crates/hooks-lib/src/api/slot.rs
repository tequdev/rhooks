//! Memory slot operations: loading ledger objects into numbered slots and
//! navigating/serializing them.
//!
//! Slot numbers and field/array indices are plain `u32` in v1 (no newtype
//! ceremony, per DESIGN.md §5.2). Functions that report a count or size
//! (`slot_count`, `slot_size`) consistently return `Result<u32>` in this
//! module.

use crate::error::{Result, res};

/// Serialize the object in `slot_no` into `out`. Returns the number of
/// bytes written.
#[inline(always)]
pub fn slot(out: &mut [u8], slot_no: u32) -> Result<usize> {
    res(unsafe { hooks_core::slot(out.as_mut_ptr() as u32, out.len() as u32, slot_no) })
        .map(|v| v as usize)
}

/// Serialize the object in `slot_no` and return it as a big-endian `u64`
/// ("as-int64" mode: `write_ptr = 0, write_len = 0`; only for data of at
/// most 8 bytes with the top bit clear, else
/// [`crate::error::HookError::TooBig`] — see `state_u64` for details).
#[inline(always)]
pub fn slot_u64(slot_no: u32) -> Result<u64> {
    res(unsafe { hooks_core::slot(0, 0, slot_no) }).map(|v| v as u64)
}

/// Free `slot_no`, making it available for reuse.
#[inline(always)]
pub fn slot_clear(slot_no: u32) -> Result<i64> {
    res(unsafe { hooks_core::slot_clear(slot_no) })
}

/// The number of array elements held in `slot_no` (the slot must hold an
/// array).
#[inline(always)]
pub fn slot_count(slot_no: u32) -> Result<u32> {
    res(unsafe { hooks_core::slot_count(slot_no) }).map(|v| v as u32)
}

/// Load an object identified by a Keylet or transaction hash (`data`) into
/// `slot_into` (`0` auto-assigns). Returns the assigned slot number.
#[inline(always)]
pub fn slot_set(data: &[u8], slot_into: u32) -> Result<u32> {
    res(unsafe { hooks_core::slot_set(data.as_ptr() as u32, data.len() as u32, slot_into) })
        .map(|v| v as u32)
}

/// The serialized size, in bytes, of the object held in `slot_no`.
#[inline(always)]
pub fn slot_size(slot_no: u32) -> Result<u32> {
    res(unsafe { hooks_core::slot_size(slot_no) }).map(|v| v as u32)
}

/// Extract element `array_id` of the array in `parent_slot` into `new_slot`
/// (`0` auto-assigns). Returns the assigned slot number.
#[inline(always)]
pub fn slot_subarray(parent_slot: u32, array_id: u32, new_slot: u32) -> Result<u32> {
    res(unsafe { hooks_core::slot_subarray(parent_slot, array_id, new_slot) }).map(|v| v as u32)
}

/// Extract field `field_id` of the object in `parent_slot` into `new_slot`
/// (`0` auto-assigns). Returns the assigned slot number.
#[inline(always)]
pub fn slot_subfield(parent_slot: u32, field_id: u32, new_slot: u32) -> Result<u32> {
    res(unsafe { hooks_core::slot_subfield(parent_slot, field_id, new_slot) }).map(|v| v as u32)
}

/// The type of the object in `slot_no`: with `flags = 0`, the field code;
/// with `flags = 1`, whether it is a native (XRP/XAH) amount.
#[inline(always)]
pub fn slot_type(slot_no: u32, flags: u32) -> Result<u32> {
    res(unsafe { hooks_core::slot_type(slot_no, flags) }).map(|v| v as u32)
}

/// Load the current transaction's metadata into `slot_into` (`0`
/// auto-assigns). Returns the assigned slot number.
#[inline(always)]
pub fn meta_slot(slot_into: u32) -> Result<u32> {
    res(unsafe { hooks_core::meta_slot(slot_into) }).map(|v| v as u32)
}

/// Load an XPOP's transaction and metadata into the given slots.
#[inline(always)]
pub fn xpop_slot(slot_no_tx: u32, slot_no_meta: u32) -> Result<i64> {
    res(unsafe { hooks_core::xpop_slot(slot_no_tx, slot_no_meta) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HookError;

    #[test]
    fn smoke_not_implemented_on_host() {
        let mut out = [0u8; 32];
        assert_eq!(slot(&mut out, 1), Err(HookError::NotImplemented));
        assert_eq!(slot_u64(1), Err(HookError::NotImplemented));
        assert_eq!(slot_clear(1), Err(HookError::NotImplemented));
        assert_eq!(slot_count(1), Err(HookError::NotImplemented));
        assert_eq!(slot_set(&out, 0), Err(HookError::NotImplemented));
        assert_eq!(slot_size(1), Err(HookError::NotImplemented));
        assert_eq!(slot_subarray(1, 0, 0), Err(HookError::NotImplemented));
        assert_eq!(slot_subfield(1, 0, 0), Err(HookError::NotImplemented));
        assert_eq!(slot_type(1, 0), Err(HookError::NotImplemented));
        assert_eq!(meta_slot(0), Err(HookError::NotImplemented));
        assert_eq!(xpop_slot(1, 2), Err(HookError::NotImplemented));
    }
}
