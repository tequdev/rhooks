//! `hooks-core` — zero-logic FFI layer for Xahau Hooks.
//!
//! A faithful, mechanical translation of the xahaud `hook/` C headers into
//! Rust: raw Hook API declarations (`api.rs`) and every constant from
//! `error.h`, `sfcodes.h`, `tts.h`, `ls_flags.h`, `tx_flags.h`, and the
//! constant-like defines of `hookapi.h`/`macro.h` (`consts.rs`).
//!
//! Upstream source: `Xahau/xahaud`, branch `release`, directory `hook/`,
//! vendored verbatim at `crates/hooks-core/vendor/xahaud-hook/` (see that
//! directory's `VENDOR.md`) and parity-tested against this translation in
//! `tests/`.
//!
//! `#![no_std]`, zero dependencies, zero logic — this crate is a translation
//! layer, not an ergonomic API; see `hooks-lib` for the idiomatic wrapper
//! that Hook developers are expected to use directly.
//!
//! Names are kept verbatim from C (`sfAccount`, `ttPAYMENT`,
//! `lsfGlobalFreeze`, `OUT_OF_BOUNDS`, ...) so hook source can be grepped
//! against the official docs and existing C hooks. All C-verbatim items are
//! re-exported at the crate root.

#![no_std]
#![allow(non_upper_case_globals)]

pub mod api;
pub mod consts;
pub mod error;
pub mod ls_flags;
pub mod sfcodes;
pub mod tts;
pub mod tx_flags;

pub use api::*;
pub use consts::*;
pub use error::*;
pub use ls_flags::*;
pub use sfcodes::*;
pub use tts::*;
pub use tx_flags::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_sfcodes_match_header() {
        assert_eq!(sfAccount, (8u32 << 16) + 1);
        assert_eq!(sfFlags, (2u32 << 16) + 2);
    }

    #[test]
    fn known_tts_match_header() {
        assert_eq!(ttPAYMENT, 0);
        assert_eq!(ttHOOK_SET, 22);
    }

    #[test]
    fn known_errors_match_header() {
        assert_eq!(SUCCESS, 0);
        assert_eq!(OUT_OF_BOUNDS, -1);
        assert_eq!(INVALID_FLOAT, -10024);
        assert_eq!(NOT_IMPLEMENTED, -14);
    }

    #[test]
    fn known_ls_flags_match_header() {
        assert_eq!(lsfGlobalFreeze, 0x0040_0000);
    }

    #[test]
    fn known_tx_flags_match_header() {
        assert_eq!(tfFullyCanonicalSig, 0x8000_0000);
        assert_eq!(tfMPTCanLock, ls_flags::lsfMPTCanLock);
    }

    #[test]
    fn known_consts_match_header() {
        assert_eq!(KEYLET_HOOK, 1);
        assert_eq!(KEYLET_CRON, 26);
        assert_eq!(tfCANONICAL, 0x8000_0000);
    }
}
