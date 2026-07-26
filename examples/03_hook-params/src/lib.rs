//! `hook-params` — a Payment filter whose minimum-amount threshold is
//! configurable via a Hook parameter (`MIN`), falling back to a baked-in
//! default when the parameter isn't set. Rolls the transaction back if the
//! originating transaction's native (XRP/XAH) `Amount` is below the
//! threshold; accepts otherwise.
//!
//! Build: `hooks-build build --manifest-path examples/03_hook-params/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, hook, hook_errors, rollback};

/// Name of the Hook parameter carrying the minimum-amount threshold, as 8
/// raw bytes, big-endian `u64` drops (see the README for the exact hex
/// encoding and how to pass it in a `SetHook` transaction).
const MIN_PARAM: &[u8] = b"MIN";

/// Threshold used when `MIN` isn't configured: 1,000,000 drops (1 XAH).
const DEFAULT_MIN_DROPS: u64 = 1_000_000;

/// Bits reserved by the native-Amount serialization format (bit 63: "not an
/// IOU"; bit 62: sign, always `1` for a native amount since XRP/XAH amounts
/// are never negative) — see `hooks_lib::txn::codec::encode_native_amount`,
/// which sets exactly these two bits when encoding a drops value the other
/// way. Masking them off here recovers the drops magnitude.
const NATIVE_AMOUNT_FLAG_BITS: u64 = 0xC000_0000_0000_0000;

hook_errors! {
    /// `hook-params` rollback codes.
    pub enum HookParamsError {
        /// The originating transaction's `Amount` isn't an 8-byte native
        /// (XRP/XAH) amount.
        UnsupportedAmount = 1,
        /// The native `Amount` fell below the configured (or default)
        /// minimum.
        BelowMinimum = 2,
    }
}

/// Read the configured minimum-amount threshold (in drops) from the `MIN`
/// Hook parameter, or [`DEFAULT_MIN_DROPS`] if it isn't set (or is the
/// wrong size to be a valid 8-byte value).
fn min_drops() -> u64 {
    hook_param_exact(MIN_PARAM)
        .map(u64::from_be_bytes)
        .unwrap_or(DEFAULT_MIN_DROPS)
}

/// Hook entry point. Reads the originating transaction's native `Amount`,
/// compares it against the configured (or default) minimum, and rolls back
/// if it falls short.
#[hook]
fn my_hook() -> i64 {
    // Only native (XRP/XAH) amounts are 8 bytes on the wire; a 48-byte IOU
    // amount would fail this length check and fall into the `Err` arm
    // below. This example deliberately doesn't handle IOU amounts — see
    // `examples/07_xfl-math` for reading *any* Amount kind uniformly via XFL.
    let drops = match otxn_field_exact(sfAmount) {
        Ok(raw) => u64::from_be_bytes(raw) & !NATIVE_AMOUNT_FLAG_BITS,
        Err(_) => rollback!(
            b"hook-params: unsupported (non-native) Amount",
            HookParamsError::UnsupportedAmount
        ),
    };

    if drops < min_drops() {
        rollback!(
            b"hook-params: amount below configured minimum",
            HookParamsError::BelowMinimum
        );
    }

    accept!()
}
