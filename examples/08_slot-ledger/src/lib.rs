//! `slot-ledger` — walks the originating transaction through the Slot API:
//! `otxn_slot` loads the whole transaction into a slot, then
//! `slot_subfield` navigates to individual fields (`sfAmount`,
//! `sfDestination`), each read out with `slot_exact`. Demonstrates
//! slot-based field access as an alternative to `otxn_field`, useful when a
//! hook needs to navigate into nested/array structure that `otxn_field`
//! alone can't reach.
//!
//! Build: `hooks-build build --manifest-path examples/08_slot-ledger/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, hook, hook_errors, rollback};

hook_errors! {
    /// `slot-ledger` rollback codes.
    pub enum SlotLedgerError {
        /// `otxn_slot` failed to load the originating transaction into a
        /// slot.
        OtxnSlotFailed = 1,
        /// `slot_subfield` found no `Destination` field on the originating
        /// transaction.
        NoDestinationField = 2,
        /// `Destination`'s slot didn't serialize to exactly 20 bytes.
        UnexpectedDestinationSize = 3,
        /// `slot_subfield` found no `Amount` field on the originating
        /// transaction.
        NoAmountField = 4,
        /// `Amount` isn't an 8-byte native (XRP/XAH) amount.
        UnsupportedAmount = 5,
        /// `slot_size` failed for the `Amount` slot.
        SlotSizeFailed = 6,
        /// Reading `Amount` out of its slot failed after `slot_size`
        /// already reported the native-amount length.
        AmountReadFailed = 7,
    }
}

/// Hook entry point. Reads `sfDestination` and `sfAmount` off the
/// originating transaction via slot navigation; accepts if both are
/// present and `Amount` is a native (8-byte) amount, rolling back
/// otherwise.
#[hook]
fn my_hook() -> i64 {
    // `slot_into = 0` auto-assigns a slot number; the return value is that
    // assigned number, used as the `parent_slot` for every subfield lookup
    // below.
    let txn_slot = match otxn_slot(0) {
        Ok(s) => s,
        Err(_) => rollback!(
            b"slot-ledger: otxn_slot failed",
            SlotLedgerError::OtxnSlotFailed
        ),
    };

    let dest_slot = match slot_subfield(txn_slot, sfDestination, 0) {
        Ok(s) => s,
        // Not every transaction type has a `Destination` field; a `Payment`
        // does, but e.g. an `AccountSet` doesn't.
        Err(_) => rollback!(
            b"slot-ledger: no Destination field on otxn",
            SlotLedgerError::NoDestinationField
        ),
    };
    let dest: AccountId = match slot_exact::<ACC_ID_LEN>(dest_slot) {
        Ok(d) => d,
        Err(_) => rollback!(
            b"slot-ledger: Destination has unexpected size",
            SlotLedgerError::UnexpectedDestinationSize
        ),
    };

    let amount_slot = match slot_subfield(txn_slot, sfAmount, 0) {
        Ok(s) => s,
        Err(_) => rollback!(
            b"slot-ledger: no Amount field on otxn",
            SlotLedgerError::NoAmountField
        ),
    };

    // `slot_size` reports the serialized size *without* copying anything
    // out yet — a native amount is 8 bytes on the wire, an IOU amount 48.
    // Checking this first means the buffer below only ever needs to be
    // sized for the native case this example actually supports (see
    // `examples/07_xfl-math` for handling both uniformly via XFL), rather
    // than always allocating room for the larger IOU encoding.
    match slot_size(amount_slot) {
        Ok(n) if n as usize == NATIVE_AMOUNT_LEN => {}
        Ok(_) => rollback!(
            b"slot-ledger: unsupported (non-native) Amount",
            SlotLedgerError::UnsupportedAmount
        ),
        Err(_) => rollback!(
            b"slot-ledger: slot_size failed for Amount",
            SlotLedgerError::SlotSizeFailed
        ),
    }

    let amount_buf: NativeAmount = match slot_exact::<NATIVE_AMOUNT_LEN>(amount_slot) {
        Ok(a) => a,
        Err(_) => rollback!(
            b"slot-ledger: reading Amount from its slot failed",
            SlotLedgerError::AmountReadFailed
        ),
    };

    // Slots are a limited resource; free them once done with them (see
    // `docs/DESIGN.md`'s Slot API notes). Cleanup failure doesn't itself
    // warrant a rollback, so each `Result` is explicitly discarded rather
    // than silently ignored.
    let _ = slot_clear(amount_slot);
    let _ = slot_clear(dest_slot);
    let _ = slot_clear(txn_slot);

    // `dest`'s and `amount_buf`'s first bytes, folded into the accept code
    // purely to prove both were actually read (real hook logic would
    // inspect the whole 20/8 bytes, e.g. against an allow-list — see
    // `firewall` — or decode the amount, e.g. `examples/03_hook-params`).
    let marker = u16::from(dest[0]).wrapping_add(u16::from(amount_buf[0]));
    accept!(
        b"slot-ledger: read Destination and native Amount",
        i64::from(marker)
    )
}
