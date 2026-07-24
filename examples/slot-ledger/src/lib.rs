//! `slot-ledger` — walks the originating transaction through the Slot API:
//! `otxn_slot` loads the whole transaction into a slot, then
//! `slot_subfield` navigates to individual fields (`sfAmount`,
//! `sfDestination`), each read out with a plain `slot` call. Demonstrates
//! slot-based field access as an alternative to `otxn_field`, useful when a
//! hook needs to navigate into nested/array structure that `otxn_field`
//! alone can't reach.
//!
//! Build: `hooks-build build --manifest-path examples/slot-ledger/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, rollback};

/// Hook entry point. Reads `sfDestination` and `sfAmount` off the
/// originating transaction via slot navigation; accepts if both are
/// present and `Amount` is a native (8-byte) amount, rolling back
/// otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn hook(_reserved: u32) -> i64 {
    // `slot_into = 0` auto-assigns a slot number; the return value is that
    // assigned number, used as the `parent_slot` for every subfield lookup
    // below.
    let txn_slot = match otxn_slot(0) {
        Ok(s) => s,
        Err(_) => rollback!(b"slot-ledger: otxn_slot failed", -1),
    };

    let dest_slot = match slot_subfield(txn_slot, sfDestination, 0) {
        Ok(s) => s,
        // Not every transaction type has a `Destination` field; a `Payment`
        // does, but e.g. an `AccountSet` doesn't.
        Err(_) => rollback!(b"slot-ledger: no Destination field on otxn", -1),
    };
    let mut dest: AccountId = [0u8; ACC_ID_LEN];
    match slot(&mut dest, dest_slot) {
        Ok(n) if n == ACC_ID_LEN => {}
        _ => rollback!(b"slot-ledger: Destination has unexpected size", -1),
    }

    let amount_slot = match slot_subfield(txn_slot, sfAmount, 0) {
        Ok(s) => s,
        Err(_) => rollback!(b"slot-ledger: no Amount field on otxn", -1),
    };

    // `slot_size` reports the serialized size *without* copying anything
    // out yet — a native amount is 8 bytes on the wire, an IOU amount 48.
    // Checking this first means the buffer below only ever needs to be
    // sized for the native case this example actually supports (see
    // `examples/xfl-math` for handling both uniformly via XFL), rather
    // than always allocating room for the larger IOU encoding.
    match slot_size(amount_slot) {
        Ok(n) if n as usize == NATIVE_AMOUNT_LEN => {}
        Ok(_) => rollback!(b"slot-ledger: unsupported (non-native) Amount", -1),
        Err(_) => rollback!(b"slot-ledger: slot_size failed for Amount", -1),
    }

    let mut amount_buf: NativeAmount = [0u8; NATIVE_AMOUNT_LEN];
    match slot(&mut amount_buf, amount_slot) {
        Ok(n) if n == NATIVE_AMOUNT_LEN => {}
        _ => rollback!(b"slot-ledger: reading Amount from its slot failed", -1),
    }

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
    // `firewall` — or decode the amount, e.g. `examples/hook-params`).
    let marker = u16::from(dest[0]).wrapping_add(u16::from(amount_buf[0]));
    accept!(
        b"slot-ledger: read Destination and native Amount",
        i64::from(marker)
    )
}
