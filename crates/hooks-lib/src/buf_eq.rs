//! Loop-free fixed-size buffer equality.
//!
//! `[u8; N] == [u8; N]` (and `<[u8]>::eq`) is not safe to use in Hook code:
//! on `wasm32v1-none` at `opt-level = "z"` (no bulk-memory instructions),
//! LLVM lowers array/slice equality to a call into `compiler_builtins`'
//! `bcmp`, which is a real, unguarded `loop` construct that never appears in
//! the Rust source (`docs/DESIGN.md` §6.3). `hooks-build`'s guard pass
//! rejects it by default; the documented workaround, `--auto-guard
//! --default-maxiter <N>`, still leaves a landmine — get the maxiter wrong
//! (too small) and the build succeeds while the *on-chain* execution can
//! still overrun it and hit `GUARD_VIOLATION`. That is the worst kind of bug
//! in this toolchain: no source-level loop, a clean build, and a runtime
//! failure that only shows up once a hook is live.
//!
//! The functions here give hook authors a way to compare fixed-size buffers
//! (`AccountId`, `Hash`, `Keylet`, ...) that never needs a guard at all,
//! because it never compiles to a loop in the first place.
//!
//! ## Word-at-a-time, not byte-at-a-time
//!
//! Each buffer is split into a fixed sequence of word-sized chunks (`u64`,
//! and a single narrower tail word — `u32`/`u16`/`u8` — for sizes that
//! aren't a multiple of 8), every chunk built from source-level *literal*
//! byte indices via `u64::from_ne_bytes`/`u32::from_ne_bytes`/etc. Because
//! this is an equality check, byte order doesn't matter as long as both
//! sides use the same order, so native-endian assembly (`from_ne_bytes`) is
//! fine and free — no `to_be`/`to_le` shuffle. Each chunk pair is XOR'd,
//! widened to `u64`, and OR-accumulated; the check at the end is `acc ==
//! 0`. This is still exactly the branchless XOR/OR fold described below,
//! just done 8 (or 4/2) bytes at a time instead of 1, which is what buys
//! the reduction in worst-case instruction count (WCE) over the original
//! byte-at-a-time version: e.g. a 32-byte compare drops from 32
//! byte-loads-per-side to 4 `i64.load`s per side.
//!
//! The chunking per size:
//!
//! | function      | size | chunks                              |
//! |---------------|------|--------------------------------------|
//! | `buf_eq_8`    | 8    | `u64`                                 |
//! | `buf_eq_20`   | 20   | `u64`, `u64`, `u32`                    |
//! | `buf_eq_32`   | 32   | `u64` × 4                              |
//! | `buf_eq_33`   | 33   | `u64` × 4, `u8`                        |
//! | `buf_eq_34`   | 34   | `u64` × 4, `u16`                       |
//! | `buf_eq_40`   | 40   | `u64` × 5                              |
//! | `buf_eq_48`   | 48   | `u64` × 6                              |
//! | `buf_eq_64`   | 64   | `u64` × 8                              |
//!
//! Every byte index that feeds a `from_ne_bytes` call is a source-level
//! literal, so — same as the byte-at-a-time version this replaces —
//! indexing is statically proven in-bounds (no panic path) and there is
//! nothing loop-shaped for LLVM to emit. `wasm32v1-none` has no alignment
//! requirement on `i32.load`/`i64.load` (unaligned access is a hint, not a
//! fault — see the `hook-rust-build` skill), so building a word from
//! byte-literal indices this way is free to lower to a single unaligned
//! load regardless of the buffer's actual alignment.
//!
//! ## Why concrete functions, not `buf_eq<const N: usize>`
//!
//! A single generic `fn buf_eq<const N: usize>(a: &[u8; N], b: &[u8; N]) ->
//! bool` was considered and rejected. Its body has exactly two possible
//! shapes:
//!
//! - A `for`/`while` loop over `0..N`. Even though `N` is a compile-time
//!   constant at every monomorphized call site, whether LLVM actually
//!   *unrolls* that loop is an optimizer heuristic — and `opt-level = "z"`
//!   is tuned to minimize code size, i.e. exactly the setting under which
//!   loop unrolling is least likely to happen. This is the same failure mode
//!   this module exists to avoid, just moved from `==` into hand-written
//!   Rust; it would still need a guard, and still leave a maxiter-sizing
//!   trap.
//! - Indexing with a runtime-computed offset into `[u8; N]` where `N` is a
//!   generic parameter: the compiler cannot prove any given index is in
//!   bounds at compile time (only literal indices into an array of a
//!   *concrete* length get that proof), so every access would carry a
//!   runtime bounds check — a panic path, which `docs/DESIGN.md` §2 C7
//!   forbids by construction. The same reasoning applies to slicing off a
//!   word-sized chunk at a generic offset.
//!
//! A macro-generated concrete function per length sidesteps both problems:
//! every byte index below is a source-level literal, so indexing is
//! statically proven in-bounds (no panic path) and there is nothing
//! loop-shaped for LLVM to emit in the first place — the comparison is
//! genuinely straight-line code, word-sized XOR-and-fold, guaranteed
//! loop-free independent of any optimizer heuristic.
//!
//! Eight lengths are provided, one per protocol-fixed buffer size in
//! [`crate::types`]: [`NATIVE_AMOUNT_LEN`](crate::types::NATIVE_AMOUNT_LEN)
//! (8), [`ACC_ID_LEN`](crate::types::ACC_ID_LEN) /
//! [`CURRENCY_CODE_LEN`](crate::types::CURRENCY_CODE_LEN) (20),
//! [`HASH_LEN`](crate::types::HASH_LEN) /
//! [`STATE_KEY_LEN`](crate::types::STATE_KEY_LEN) /
//! [`NAMESPACE_LEN`](crate::types::NAMESPACE_LEN) /
//! [`NONCE_LEN`](crate::types::NONCE_LEN) (32),
//! [`PUB_KEY_LEN`](crate::types::PUB_KEY_LEN) (33),
//! [`KEYLET_LEN`](crate::types::KEYLET_LEN) (34), 40, 48
//! ([`IOU_AMOUNT_LEN`](crate::types::IOU_AMOUNT_LEN)), and 64, covering
//! other common fixed-size buffers (e.g. two concatenated `Hash`es, or a
//! 512-bit digest) that don't have a dedicated named constant in
//! [`crate::types`]. A hook comparing a buffer of some other fixed size can
//! follow the same pattern by hand: word-sized chunks built from literal
//! indices via `from_ne_bytes`, XOR'd and OR-folded, `== 0` at the end.

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
