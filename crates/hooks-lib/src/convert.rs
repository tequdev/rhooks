//! Boundary conversion traits: [`ToBytes`]/[`FromBytes`].
//!
//! These traits fix exactly how a small, fixed-size Rust value crosses the
//! boundary into/out of a protocol byte buffer (a hook state entry, a
//! `state_keys!`-encoded key payload, ...). They generalize the "little-
//! endian, fixed layout" convention this crate already documents for
//! [`crate::api::state::state_u64`]'s *underlying state entries* (as
//! opposed to that function's own big-endian "as-int64" wire encoding —
//! see its doc comment) so the typed storage layer (`crate::state`'s
//! `state_get`/`state_set_typed`/`state_update_typed`) can encode/decode
//! arbitrary fixed-size types without repeating that logic per call site.
//!
//! # Implementor's contract
//!
//! Every impl of [`ToBytes`]/[`FromBytes`] — the ones in this module, the
//! newtype impls in `types.rs`, and any a hook crate adds for its own
//! types — must stay panic-free, loop-free, and heap-free, the same
//! constraints as every other hooks-lib wrapper (DESIGN.md §2 C2/C7):
//! `wasm32v1-none` hook binaries have no allocator, and an unguarded loop
//! fails the Hook API's guard checker. Concretely:
//!
//! - Never index with `buf[i]` (this crate denies `clippy::indexing_slicing`
//!   crate-wide) — use `.get()`/`.get_mut()` over a range whose bounds are
//!   compile-time constants, then `copy_from_slice`. Keeping the range
//!   compile-time-constant (never a runtime-computed length) is what keeps
//!   the copy a handful of inlined loads/stores instead of a lowering to a
//!   `memcpy`/`memcmp` call with a runtime length — exactly the std idiom
//!   DESIGN.md warns produces unguardable loops in a Hook binary.
//! - [`ToBytes::MAX_LEN`] must be a compile-time constant equal to the
//!   exact number of bytes a successful [`ToBytes::write`] produces.
//! - [`ToBytes::write`] must not panic if `buf` is shorter than `MAX_LEN`:
//!   write nothing and return `0` instead (mirrors this crate's other
//!   caller-buffer wrappers, which rely on the host's own bounds checking
//!   rather than panicking locally).

use crate::error::{HookError, Result};

/// Encode `Self` into the front of a caller-provided buffer.
///
/// Mirrors this crate's caller-buffer convention (`state`, `hook_account`,
/// ...): implementations never allocate and never panic. See the module
/// doc comment for the loop-free/panic-free contract every impl must
/// uphold.
pub trait ToBytes {
    /// The exact number of bytes a successful [`ToBytes::write`] produces.
    const MAX_LEN: usize;

    /// Write `self`'s encoding into `buf[..Self::MAX_LEN]`.
    ///
    /// Returns `Self::MAX_LEN` (the number of bytes written) on success, or
    /// `0` if `buf` is shorter than `Self::MAX_LEN` (nothing is written in
    /// that case — never a partial write).
    fn write(&self, buf: &mut [u8]) -> usize;
}

/// Decode `Self` from a byte buffer.
pub trait FromBytes: Sized {
    /// Decode `Self` from `buf`.
    ///
    /// # Errors
    ///
    /// Returns [`HookError::TooSmall`] if `buf` is shorter than the
    /// encoding this type expects.
    fn read(buf: &[u8]) -> Result<Self>;
}

impl ToBytes for u32 {
    const MAX_LEN: usize = 4;

    #[inline(always)]
    fn write(&self, buf: &mut [u8]) -> usize {
        match buf.get_mut(..4) {
            Some(dst) => {
                dst.copy_from_slice(&self.to_le_bytes());
                4
            }
            None => 0,
        }
    }
}

impl FromBytes for u32 {
    #[inline(always)]
    fn read(buf: &[u8]) -> Result<Self> {
        let src = buf.get(..4).ok_or(HookError::TooSmall)?;
        let mut arr = [0u8; 4];
        arr.copy_from_slice(src);
        Ok(u32::from_le_bytes(arr))
    }
}

impl ToBytes for u64 {
    const MAX_LEN: usize = 8;

    #[inline(always)]
    fn write(&self, buf: &mut [u8]) -> usize {
        match buf.get_mut(..8) {
            Some(dst) => {
                dst.copy_from_slice(&self.to_le_bytes());
                8
            }
            None => 0,
        }
    }
}

impl FromBytes for u64 {
    #[inline(always)]
    fn read(buf: &[u8]) -> Result<Self> {
        let src = buf.get(..8).ok_or(HookError::TooSmall)?;
        let mut arr = [0u8; 8];
        arr.copy_from_slice(src);
        Ok(u64::from_le_bytes(arr))
    }
}

impl ToBytes for i64 {
    const MAX_LEN: usize = 8;

    #[inline(always)]
    fn write(&self, buf: &mut [u8]) -> usize {
        match buf.get_mut(..8) {
            Some(dst) => {
                dst.copy_from_slice(&self.to_le_bytes());
                8
            }
            None => 0,
        }
    }
}

impl FromBytes for i64 {
    #[inline(always)]
    fn read(buf: &[u8]) -> Result<Self> {
        let src = buf.get(..8).ok_or(HookError::TooSmall)?;
        let mut arr = [0u8; 8];
        arr.copy_from_slice(src);
        Ok(i64::from_le_bytes(arr))
    }
}

impl ToBytes for crate::xfl::XFL {
    // An XFL is an opaque wrapper over a raw `i64` bit pattern (see
    // `xfl.rs`), so it shares `i64`'s width and little-endian convention.
    const MAX_LEN: usize = 8;

    #[inline(always)]
    fn write(&self, buf: &mut [u8]) -> usize {
        self.raw_bits().write(buf)
    }
}

impl FromBytes for crate::xfl::XFL {
    #[inline(always)]
    fn read(buf: &[u8]) -> Result<Self> {
        i64::read(buf).map(crate::xfl::XFL::from_raw_bits)
    }
}

impl<const N: usize> ToBytes for [u8; N] {
    const MAX_LEN: usize = N;

    #[inline(always)]
    fn write(&self, buf: &mut [u8]) -> usize {
        match buf.get_mut(..N) {
            Some(dst) => {
                dst.copy_from_slice(self);
                N
            }
            None => 0,
        }
    }
}

impl<const N: usize> FromBytes for [u8; N] {
    #[inline(always)]
    fn read(buf: &[u8]) -> Result<Self> {
        let src = buf.get(..N).ok_or(HookError::TooSmall)?;
        let mut out = [0u8; N];
        out.copy_from_slice(src);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u32_round_trips() {
        let mut buf = [0u8; 4];
        assert_eq!(42u32.write(&mut buf), 4);
        assert_eq!(buf, 42u32.to_le_bytes());
        assert_eq!(u32::read(&buf), Ok(42u32));
    }

    #[test]
    fn u32_write_into_short_buffer_writes_nothing() {
        let mut buf = [0xFFu8; 3];
        assert_eq!(42u32.write(&mut buf), 0);
        assert_eq!(buf, [0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn u32_read_from_short_buffer_fails() {
        assert_eq!(u32::read(&[0u8; 3]), Err(HookError::TooSmall));
    }

    #[test]
    fn u64_round_trips() {
        let mut buf = [0u8; 8];
        assert_eq!(0x0102_0304_0506_0708u64.write(&mut buf), 8);
        assert_eq!(u64::read(&buf), Ok(0x0102_0304_0506_0708u64));
    }

    #[test]
    fn i64_round_trips() {
        let mut buf = [0u8; 8];
        assert_eq!((-1i64).write(&mut buf), 8);
        assert_eq!(i64::read(&buf), Ok(-1i64));
    }

    #[test]
    fn fixed_array_round_trips() {
        let value = [1u8, 2, 3, 4, 5];
        let mut buf = [0u8; 5];
        assert_eq!(value.write(&mut buf), 5);
        assert_eq!(buf, value);
        assert_eq!(<[u8; 5]>::read(&buf), Ok(value));
    }

    #[test]
    fn xfl_round_trips_bit_pattern() {
        use crate::xfl::XFL;

        let value = XFL::from_raw_bits(0x1234_5678_9ABC_DEF0);
        let mut buf = [0u8; 8];
        assert_eq!(value.write(&mut buf), 8);
        assert_eq!(
            crate::xfl::XFL::read(&buf).map(XFL::raw_bits),
            Ok(0x1234_5678_9ABC_DEF0i64)
        );
    }
}
