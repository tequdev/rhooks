//! Loop-free equality checks for protocol-sized byte arrays.
//!
//! These concrete functions use literal indices and straight-line XOR/OR
//! comparisons so Hooks do not rely on compiler-generated comparison loops.

/// Computes `u64((a's word at literal indices) ^ (b's word at literal
/// indices))` for one word-sized chunk, where the word width is `u64`,
/// `u32`, `u16`, or `u8`. Every index is a source-level literal, so this is
/// panic-free (statically proven in-bounds) and loop-free (straight-line).
macro_rules! word_diff {
    ($a:ident, $b:ident, u64, [$($i:literal),+ $(,)?]) => {
        u64::from_ne_bytes([$($a[$i]),+]) ^ u64::from_ne_bytes([$($b[$i]),+])
    };
    ($a:ident, $b:ident, $ty:ident, [$($i:literal),+ $(,)?]) => {
        ($ty::from_ne_bytes([$($a[$i]),+]) ^ $ty::from_ne_bytes([$($b[$i]),+])) as u64
    };
}

/// Generates a loop-free, panic-free equality function for a `$n`-byte
/// buffer, comparing it as a fixed sequence of word-sized chunks (see the
/// [module docs](self)). Every index in every chunk is a literal, so the
/// body is straight-line code (no loop, no bounds-check panic path)
/// regardless of optimization level.
macro_rules! impl_buf_eq {
    ($name:ident, $n:literal, [ $( $ty:ident [ $($i:literal),+ $(,)? ] ),+ $(,)? ]) => {
        #[doc = concat!(
            "Loop-free, panic-free equality check for two ", stringify!($n),
            "-byte buffers. See the [module docs](self) for why this exists ",
            "instead of `a == b`."
        )]
        #[inline(always)]
        #[must_use]
        pub fn $name(a: &[u8; $n], b: &[u8; $n]) -> bool {
            let mut acc: u64 = 0;
            $( acc |= word_diff!(a, b, $ty, [$($i),+]); )+
            acc == 0
        }
    };
}

impl_buf_eq!(buf_eq_8, 8, [u64[0, 1, 2, 3, 4, 5, 6, 7]]);
impl_buf_eq!(
    buf_eq_20,
    20,
    [
        u64[0, 1, 2, 3, 4, 5, 6, 7],
        u64[8, 9, 10, 11, 12, 13, 14, 15],
        u32[16, 17, 18, 19],
    ]
);
impl_buf_eq!(
    buf_eq_32,
    32,
    [
        u64[0, 1, 2, 3, 4, 5, 6, 7],
        u64[8, 9, 10, 11, 12, 13, 14, 15],
        u64[16, 17, 18, 19, 20, 21, 22, 23],
        u64[24, 25, 26, 27, 28, 29, 30, 31],
    ]
);
impl_buf_eq!(
    buf_eq_33,
    33,
    [
        u64[0, 1, 2, 3, 4, 5, 6, 7],
        u64[8, 9, 10, 11, 12, 13, 14, 15],
        u64[16, 17, 18, 19, 20, 21, 22, 23],
        u64[24, 25, 26, 27, 28, 29, 30, 31],
        u8[32],
    ]
);
impl_buf_eq!(
    buf_eq_34,
    34,
    [
        u64[0, 1, 2, 3, 4, 5, 6, 7],
        u64[8, 9, 10, 11, 12, 13, 14, 15],
        u64[16, 17, 18, 19, 20, 21, 22, 23],
        u64[24, 25, 26, 27, 28, 29, 30, 31],
        u16[32, 33],
    ]
);
impl_buf_eq!(
    buf_eq_40,
    40,
    [
        u64[0, 1, 2, 3, 4, 5, 6, 7],
        u64[8, 9, 10, 11, 12, 13, 14, 15],
        u64[16, 17, 18, 19, 20, 21, 22, 23],
        u64[24, 25, 26, 27, 28, 29, 30, 31],
        u64[32, 33, 34, 35, 36, 37, 38, 39],
    ]
);
impl_buf_eq!(
    buf_eq_48,
    48,
    [
        u64[0, 1, 2, 3, 4, 5, 6, 7],
        u64[8, 9, 10, 11, 12, 13, 14, 15],
        u64[16, 17, 18, 19, 20, 21, 22, 23],
        u64[24, 25, 26, 27, 28, 29, 30, 31],
        u64[32, 33, 34, 35, 36, 37, 38, 39],
        u64[40, 41, 42, 43, 44, 45, 46, 47],
    ]
);
impl_buf_eq!(
    buf_eq_64,
    64,
    [
        u64[0, 1, 2, 3, 4, 5, 6, 7],
        u64[8, 9, 10, 11, 12, 13, 14, 15],
        u64[16, 17, 18, 19, 20, 21, 22, 23],
        u64[24, 25, 26, 27, 28, 29, 30, 31],
        u64[32, 33, 34, 35, 36, 37, 38, 39],
        u64[40, 41, 42, 43, 44, 45, 46, 47],
        u64[48, 49, 50, 51, 52, 53, 54, 55],
        u64[56, 57, 58, 59, 60, 61, 62, 63],
    ]
);

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // tests are exempt from panic-freedom lints, docs/DESIGN.md §8
    use super::*;

    /// Runs the same equal/unequal-at-every-position check that the
    /// hand-written per-size tests below exercise, generically, so every
    /// `buf_eq_*` function gets the same coverage without repeating the
    /// boilerplate per size, and cross-checks every result against `a ==
    /// b` (host-side; array `==` is what this module exists to *avoid*
    /// inside a hook, but it's the correctness oracle here).
    fn check_eq_and_all_single_byte_diffs<const N: usize>(eq: fn(&[u8; N], &[u8; N]) -> bool) {
        let a = [0x5Au8; N];
        let b = a;
        assert!(eq(&a, &b), "identical {N}-byte buffers must compare equal");
        assert_eq!(eq(&a, &b), a == b, "buf_eq_{N} must agree with a == b");

        for i in 0..N {
            let mut diff = a;
            diff[i] ^= 0xFF;
            assert!(
                !eq(&a, &diff),
                "single-byte difference at index {i} ({N} bytes) should be detected"
            );
            assert_eq!(
                eq(&a, &diff),
                a == diff,
                "buf_eq_{N} must agree with a == b at index {i}"
            );
        }
    }

    #[test]
    fn buf_eq_8_matches_slice_eq() {
        check_eq_and_all_single_byte_diffs(buf_eq_8);
    }

    #[test]
    fn buf_eq_20_matches_slice_eq() {
        check_eq_and_all_single_byte_diffs(buf_eq_20);
    }

    #[test]
    fn buf_eq_32_matches_slice_eq() {
        check_eq_and_all_single_byte_diffs(buf_eq_32);
    }

    #[test]
    fn buf_eq_33_matches_slice_eq() {
        check_eq_and_all_single_byte_diffs(buf_eq_33);
    }

    #[test]
    fn buf_eq_34_matches_slice_eq() {
        check_eq_and_all_single_byte_diffs(buf_eq_34);
    }

    #[test]
    fn buf_eq_40_matches_slice_eq() {
        check_eq_and_all_single_byte_diffs(buf_eq_40);
    }

    #[test]
    fn buf_eq_48_matches_slice_eq() {
        check_eq_and_all_single_byte_diffs(buf_eq_48);
    }

    #[test]
    fn buf_eq_64_matches_slice_eq() {
        check_eq_and_all_single_byte_diffs(buf_eq_64);
    }

    #[test]
    fn buf_eq_20_detects_difference_at_every_position() {
        let a = [0u8; 20];
        for i in 0..20 {
            let mut b = a;
            if let Some(byte) = b.get_mut(i) {
                *byte = 1;
            }
            assert!(
                !buf_eq_20(&a, &b),
                "difference at index {i} should be detected"
            );
        }
    }

    #[test]
    fn buf_eq_8_basic() {
        assert!(buf_eq_8(
            &[1, 2, 3, 4, 5, 6, 7, 8],
            &[1, 2, 3, 4, 5, 6, 7, 8]
        ));
        assert!(!buf_eq_8(
            &[1, 2, 3, 4, 5, 6, 7, 8],
            &[1, 2, 3, 4, 5, 6, 7, 9]
        ));
    }

    #[test]
    fn buf_eq_32_basic() {
        let a = [0xABu8; 32];
        let mut b = a;
        assert!(buf_eq_32(&a, &b));
        b[31] = 0xAC;
        assert!(!buf_eq_32(&a, &b));
    }

    #[test]
    fn buf_eq_33_basic() {
        let a = [0x11u8; 33];
        let mut b = a;
        assert!(buf_eq_33(&a, &b));
        b[0] = 0x12;
        assert!(!buf_eq_33(&a, &b));
    }

    #[test]
    fn buf_eq_34_basic() {
        let a = [0x22u8; 34];
        let mut b = a;
        assert!(buf_eq_34(&a, &b));
        b[17] = 0x23;
        assert!(!buf_eq_34(&a, &b));
    }

    #[test]
    fn buf_eq_40_basic() {
        let a = [0x44u8; 40];
        let mut b = a;
        assert!(buf_eq_40(&a, &b));
        b[39] = 0x45;
        assert!(!buf_eq_40(&a, &b));
    }

    #[test]
    fn buf_eq_48_basic() {
        let a = [0x33u8; 48];
        let mut b = a;
        assert!(buf_eq_48(&a, &b));
        b[47] = 0x34;
        assert!(!buf_eq_48(&a, &b));
    }

    #[test]
    fn buf_eq_64_basic() {
        let a = [0x55u8; 64];
        let mut b = a;
        assert!(buf_eq_64(&a, &b));
        b[63] = 0x56;
        assert!(!buf_eq_64(&a, &b));
    }
}
