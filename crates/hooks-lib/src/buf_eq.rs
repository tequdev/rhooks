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
//!   forbids by construction.
//!
//! A macro-generated concrete function per length sidesteps both problems:
//! every byte index below is a source-level literal, so indexing is
//! statically proven in-bounds (no panic path) and there is nothing
//! loop-shaped for LLVM to emit in the first place — the comparison is
//! genuinely straight-line code, XOR-ing each byte pair together and
//! OR-accumulating the result, guaranteed loop-free independent of any
//! optimizer heuristic.
//!
//! Six lengths are provided, one per protocol-fixed buffer size in
//! [`crate::types`]: [`NATIVE_AMOUNT_LEN`](crate::types::NATIVE_AMOUNT_LEN)
//! (8), [`ACC_ID_LEN`](crate::types::ACC_ID_LEN) /
//! [`CURRENCY_CODE_LEN`](crate::types::CURRENCY_CODE_LEN) (20),
//! [`HASH_LEN`](crate::types::HASH_LEN) /
//! [`STATE_KEY_LEN`](crate::types::STATE_KEY_LEN) /
//! [`NAMESPACE_LEN`](crate::types::NAMESPACE_LEN) /
//! [`NONCE_LEN`](crate::types::NONCE_LEN) (32),
//! [`PUB_KEY_LEN`](crate::types::PUB_KEY_LEN) (33),
//! [`KEYLET_LEN`](crate::types::KEYLET_LEN) (34), and
//! [`IOU_AMOUNT_LEN`](crate::types::IOU_AMOUNT_LEN) (48). A hook comparing a
//! buffer of some other fixed size can follow the same pattern by hand: an
//! XOR/OR fold over literal indices, `== 0` at the end.

/// Generates a loop-free, panic-free equality function for a `$n`-byte
/// buffer. Every index in the fold is a literal, so the body is
/// straight-line code (no loop, no bounds-check panic path) regardless of
/// optimization level.
macro_rules! impl_buf_eq {
    ($name:ident, $n:literal, [$($i:literal),+ $(,)?]) => {
        #[doc = concat!(
            "Loop-free, panic-free equality check for two ", stringify!($n),
            "-byte buffers. See the [module docs](self) for why this exists ",
            "instead of `a == b`."
        )]
        #[inline(always)]
        #[must_use]
        pub fn $name(a: &[u8; $n], b: &[u8; $n]) -> bool {
            let mut acc: u8 = 0;
            $( acc |= a[$i] ^ b[$i]; )+
            acc == 0
        }
    };
}

impl_buf_eq!(buf_eq_8, 8, [0, 1, 2, 3, 4, 5, 6, 7]);
impl_buf_eq!(
    buf_eq_20,
    20,
    [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19
    ]
);
impl_buf_eq!(
    buf_eq_32,
    32,
    [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31
    ]
);
impl_buf_eq!(
    buf_eq_33,
    33,
    [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31, 32
    ]
);
impl_buf_eq!(
    buf_eq_34,
    34,
    [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31, 32, 33
    ]
);
impl_buf_eq!(
    buf_eq_48,
    48,
    [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47
    ]
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buf_eq_20_matches_equal_buffers() {
        let a = [7u8; 20];
        let b = [7u8; 20];
        assert!(buf_eq_20(&a, &b));
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
    fn buf_eq_48_basic() {
        let a = [0x33u8; 48];
        let mut b = a;
        assert!(buf_eq_48(&a, &b));
        b[47] = 0x34;
        assert!(!buf_eq_48(&a, &b));
    }
}
