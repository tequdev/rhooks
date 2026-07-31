//! `slot-ledger` — walks the originating transaction through the **typed**
//! Slot API: [`SlotObject::from_otxn`] loads the whole transaction into a
//! slot, then `.get(..)` navigates to individual fields (`sfAmount`,
//! `sfDestination`), each read out with `.value()`. Demonstrates slot-based
//! field access as an alternative to `otxn_field`, useful when a hook needs
//! to navigate into nested/array structure that `otxn_field` alone can't
//! reach.
//!
//! # What the typed layer changes
//!
//! Slot numbers never appear. `SlotObject::from_otxn()` hands back a
//! `SlotObject<STObject>`; `.get(sfDestination)` hands back a
//! `SlotObject<AccountId>` — the field constant carries the value type, so
//! `.value()` needs no turbofish and no `slot_exact::<AccountId>` guess. The
//! handle is affine: `.take_value()` below both reads *and* releases the
//! slot, which is why nothing here calls `slot_clear` by number.
//!
//! It costs nothing: every wrapper is `#[inline(always)]` over the same host
//! call. Measured against a raw version making the *same* calls with the
//! *same* (no-clear) cleanup policy, this hook is byte-identical — 197
//! instructions, 925 bytes either way. See the README's "Typed vs raw"
//! table, which also shows the clearing variants.
//!
//! Build: `hooks-build build --manifest-path examples/08_slot-ledger/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, hook, hook_errors, rollback};

hook_errors! {
    /// `slot-ledger` rollback codes.
    pub enum SlotLedgerError {
        /// Loading the originating transaction into a slot failed.
        OtxnSlotFailed = 1,
        /// The originating transaction has no `Destination` field.
        NoDestinationField = 2,
        /// `Destination`'s slot didn't serialize to exactly 20 bytes.
        UnexpectedDestinationSize = 3,
        /// The originating transaction has no `Amount` field.
        NoAmountField = 4,
        /// `Amount` isn't an 8-byte native (XRP/XAH) amount.
        UnsupportedAmount = 5,
        /// Reading the `Amount` slot's size failed.
        SlotSizeFailed = 6,
        /// Reading `Amount` out of its slot failed after its size already
        /// reported the native-amount length.
        AmountReadFailed = 7,
    }
}

/// Hook entry point. Reads `sfDestination` and `sfAmount` off the
/// originating transaction via typed slot navigation; accepts if both are
/// present and `Amount` is a native (8-byte) amount, rolling back
/// otherwise.
#[hook]
fn my_hook() -> i64 {
    // No slot number anywhere: the host auto-assigns one and the handle
    // carries it. `SlotObject<STObject>` — a transaction is an object.
    let txn = match SlotObject::from_otxn() {
        Ok(s) => s,
        Err(_) => rollback!(
            b"slot-ledger: otxn_slot failed",
            SlotLedgerError::OtxnSlotFailed
        ),
    };

    // `.get` borrows `txn`, so the same handle keeps working for the second
    // field below. `sfDestination` is an `SField<AccountId>`, which is what
    // makes the binding's type inferable rather than annotated.
    let dest_slot = match txn.get(sfDestination) {
        Ok(s) => s,
        // Not every transaction type has a `Destination` field; a `Payment`
        // does, but e.g. an `AccountSet` doesn't.
        Err(_) => rollback!(
            b"slot-ledger: no Destination field on otxn",
            SlotLedgerError::NoDestinationField
        ),
    };
    // `value()` consumes the handle and does **not** clear the slot: the
    // slot stays loaded until the hook returns, at which point the host
    // frees it. That is deliberate, and it is what the C idiom costs — a C
    // `slot_subfield` + `slot()` read leaks the slot identically, and an
    // implicit clear would bill every read for a host call C never pays.
    // The raw version of this example *did* call `slot_clear` three times at
    // the end; dropping those is why the typed rewrite measures **cheaper**
    // (see the README's "Typed vs raw" table).
    //
    // A loop that derives a slot per iteration is the case that needs the
    // other form: `take_value()`/`take_xfl()`/`take_raw_exact()` read and
    // release in one go, keeping a long loop inside the 255-slot budget.
    let dest: AccountId = match dest_slot.value() {
        Ok(d) => d,
        Err(_) => rollback!(
            b"slot-ledger: Destination has unexpected size",
            SlotLedgerError::UnexpectedDestinationSize
        ),
    };

    let amount_slot = match txn.get(sfAmount) {
        Ok(s) => s,
        Err(_) => rollback!(
            b"slot-ledger: no Amount field on otxn",
            SlotLedgerError::NoAmountField
        ),
    };

    // `size()` *borrows*, which is the whole reason it does: it reports the
    // serialized size without copying anything out and without spending the
    // handle, so the consuming read below still has one. A native amount is
    // 8 bytes on the wire, an IOU 48 — checking first means the read only
    // ever handles the native case this example supports (see
    // `examples/07_xfl-math` for handling both uniformly via XFL).
    match amount_slot.size() {
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

    // The size is already known to be `NATIVE_AMOUNT_LEN`, so this reads
    // exactly that many bytes — the same single `slot` call the raw version
    // made, into the same 8-byte stack buffer.
    //
    // `.value()` is the more typed spelling: `sfAmount` is an
    // `SField<Amount>`, so it reads back as an `AmountBytes` classified by
    // length (`Native` or `Iou`, never a guess). It is *not* the same
    // operation, though — it reads into a 48-byte buffer, because an IOU
    // amount has to fit, and then branches on the length, which is redundant
    // once `size()` has already answered that question. Measured at +12
    // instructions here; the README's "Typed vs raw" table has both.
    let amount_buf: NativeAmount = match amount_slot.raw_exact::<NATIVE_AMOUNT_LEN>() {
        Ok(a) => NativeAmount(a),
        Err(_) => rollback!(
            b"slot-ledger: reading Amount from its slot failed",
            SlotLedgerError::AmountReadFailed
        ),
    };

    // `dest`'s and `amount_buf`'s first bytes, folded into the accept code
    // purely to prove both were actually read (real hook logic would
    // inspect the whole 20/8 bytes, e.g. against an allow-list — see
    // `firewall` — or decode the amount, e.g. `examples/03_hook-params`).
    // `.first()` (not `dest[0]`): indexing through the `AccountId`/
    // `NativeAmount` newtypes' `Deref` reaches a `[u8; N]`, but clippy's
    // `indexing_slicing` lint only recognizes a literal index as
    // provably-in-bounds when the receiver's own type is the array (not a
    // newtype wrapping one reached via `Deref`) — see `hooks_lib::types`'
    // module doc comment.
    let marker = u16::from(dest.first().copied().unwrap_or(0))
        .wrapping_add(u16::from(amount_buf.first().copied().unwrap_or(0)));
    accept!(
        b"slot-ledger: read Destination and native Amount",
        i64::from(marker)
    )
}
