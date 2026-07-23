//! Serialized Transaction Object (STO) manipulation: extracting fields and
//! array elements, and inserting/removing fields.
//!
//! `sto_subfield`/`sto_subarray` return a packed `(offset, length)` value
//! (upper 32 bits = offset into the source buffer, lower 32 bits = payload
//! length — see `SUB_OFFSET`/`SUB_LENGTH` in the upstream C API) as a raw
//! `Result<i64>`. Unpacking that into a typed "field pointer" is a deliberate
//! v1 scope limit, left as future work.

use crate::error::{Result, res};

/// Validate the integrity of the STO in `sto`.
#[inline(always)]
pub fn sto_validate(sto: &[u8]) -> Result<bool> {
    res(unsafe { hooks_core::sto_validate(sto.as_ptr() as u32, sto.len() as u32) }).map(|v| v != 0)
}

/// Locate field `field_id` within the STO `sto`. Returns the raw packed
/// `(offset, length)` value; see the module doc comment.
#[inline(always)]
pub fn sto_subfield(sto: &[u8], field_id: u32) -> Result<i64> {
    res(unsafe { hooks_core::sto_subfield(sto.as_ptr() as u32, sto.len() as u32, field_id) })
}

/// Locate element `index` within the STO array `array`. Returns the raw
/// packed `(offset, length)` value; see the module doc comment.
#[inline(always)]
pub fn sto_subarray(array: &[u8], index: u32) -> Result<i64> {
    res(unsafe { hooks_core::sto_subarray(array.as_ptr() as u32, array.len() as u32, index) })
}

/// Insert or replace field `field_id` (encoded as `field`) into the STO
/// `source`, writing the result to `out`. Returns the number of bytes
/// written.
#[inline(always)]
pub fn sto_emplace(out: &mut [u8], source: &[u8], field: &[u8], field_id: u32) -> Result<usize> {
    res(unsafe {
        hooks_core::sto_emplace(
            out.as_mut_ptr() as u32,
            out.len() as u32,
            source.as_ptr() as u32,
            source.len() as u32,
            field.as_ptr() as u32,
            field.len() as u32,
            field_id,
        )
    })
    .map(|v| v as usize)
}

/// Remove field `field_id` from the STO `source`, writing the result to
/// `out`. Returns the number of bytes written.
#[inline(always)]
pub fn sto_erase(out: &mut [u8], source: &[u8], field_id: u32) -> Result<usize> {
    res(unsafe {
        hooks_core::sto_erase(
            out.as_mut_ptr() as u32,
            out.len() as u32,
            source.as_ptr() as u32,
            source.len() as u32,
            field_id,
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
        let sto = [0u8; 4];
        let mut out = [0u8; 8];
        assert_eq!(sto_validate(&sto), Err(HookError::NotImplemented));
        assert_eq!(sto_subfield(&sto, 0), Err(HookError::NotImplemented));
        assert_eq!(sto_subarray(&sto, 0), Err(HookError::NotImplemented));
        assert_eq!(
            sto_emplace(&mut out, &sto, &sto, 0),
            Err(HookError::NotImplemented)
        );
        assert_eq!(sto_erase(&mut out, &sto, 0), Err(HookError::NotImplemented));
    }
}
