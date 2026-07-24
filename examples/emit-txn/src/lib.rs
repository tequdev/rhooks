//! `emit-txn` — reserves an emission slot and emits a 1-drop Payment back
//! to the account that sent the originating transaction, using its own
//! `Payment` template (declared below with `hooks_lib::txn_template!`) to
//! build the entire wire-format transaction ourselves. Also exports a
//! `cbak` callback for when the emitted transaction settles.
//!
//! Build: `hooks-build build --manifest-path examples/emit-txn/Cargo.toml`
//!
//! ## Why not `prepare()`
//!
//! `prepare()` is a HooksUpdate2+ Hook API function whose substitution
//! contract is documented only as "auto-fill system fields," with no
//! further specification. `Payment` instead mirrors xahaud's own C
//! tx-builder macro, `PREPARE_PAYMENT_SIMPLE`
//! (`hook/genesis/headers/macro.h`), fully specified here: only the fields
//! that genuinely require a host call (`FirstLedgerSequence`/
//! `LastLedgerSequence` via `ledger_seq()`, `Account` via `hook_account()`,
//! `EmitDetails` via `etxn_details()`, `Fee` via `etxn_fee_base()`) ever
//! touch undocumented host behavior — everything else is a byte-exact,
//! reviewable constant. Those fields, plus `Sequence`/`SigningPubKey`
//! (always a fixed baked constant on an emitted transaction), are declared
//! below with `txn_template!`'s ordinary, uniform kinds — there is no
//! special declaration syntax for them. `txn_template!` recognizes them by
//! their `sfXxx` code *value* and requires all six (`Sequence`,
//! `FirstLedgerSequence`, `LastLedgerSequence`, `Fee`, `SigningPubKey`,
//! `Account`) plus an `emit_details` field to be present, which makes the
//! macro generate `Payment::prepare_for_emit()` itself; this crate no
//! longer hand-writes that function. See `hooks_lib::txn`
//! (`docs/DESIGN.md` §5.5) for the generated method's exact semantics.
//!
//! ## The template lives here, not in `hooks-lib`
//!
//! See `hooks_lib::txn` and `docs/DESIGN.md` §5.5: `hooks-lib` deliberately
//! owns no transaction-shaped type, so `txn_template!` turns *this crate's*
//! field list into a byte-exact template, and adding a field never needs a
//! `hooks-lib` release.

#![no_std]
// Required by `txn_template!`: it expands `${concat(set_, $field)}` into
// this crate, and that unstable syntax must be feature-gated here too, not
// just in hooks-lib where the macro is defined.
#![feature(macro_metavar_expr_concat)]

use hooks_lib::prelude::*;
use hooks_lib::{accept, rollback, txn_template};

txn_template! {
    /// This hook's Payment template: `TransactionType` through
    /// `Destination` at fixed offsets, followed by a reserved `EmitDetails`
    /// region the host fills in. Every field uses the same uniform kinds;
    /// `sequence`, `first_ledger_sequence`, `last_ledger_sequence`, `fee`,
    /// `signing_pub_key`, and `account` are recognized as required by
    /// their `sfXxx` code, together with `emit_details`, which is what
    /// makes the macro generate `Payment::prepare_for_emit()`.
    struct Payment {
        transaction_type = ttPAYMENT,
        flags: u32_field(sfFlags) = tfCANONICAL,
        source_tag: u32_field(sfSourceTag) = 0,
        sequence: u32_field(sfSequence) = 0,
        destination_tag: u32_field(sfDestinationTag) = 0,
        first_ledger_sequence: u32_field(sfFirstLedgerSequence) = 0,
        last_ledger_sequence: u32_field(sfLastLedgerSequence) = 0,
        amount: native_amount(sfAmount) = 0,
        fee: native_amount(sfFee) = 0,
        signing_pub_key: empty_vl(sfSigningPubKey),
        account: account_id(sfAccount),
        destination: account_id(sfDestination),
        emit_details: emit_details,
    }
}

/// The Payment template, `const`-initialized so it lands in a wasm data
/// segment (see `Payment::new`'s doc comment and
/// `hooks_lib::static_cell::HookStatic`).
static TXN: HookStatic<Payment> = HookStatic::new(Payment::new());

/// Hook entry point: emits a 1-drop Payment back to the originating
/// transaction's sender.
#[unsafe(no_mangle)]
pub extern "C" fn hook(_reserved: u32) -> i64 {
    if etxn_reserve(1).is_err() {
        rollback!(b"emit-txn: etxn_reserve failed", -1);
    }

    let mut dest: AccountId = [0u8; ACC_ID_LEN];
    match otxn_field(&mut dest, sfAccount) {
        Ok(n) if n == ACC_ID_LEN => {}
        _ => rollback!(b"emit-txn: could not read otxn sender", -1),
    }

    // None only on a second take, which this hook never performs.
    let Some(txn) = TXN.take() else {
        rollback!(b"emit-txn: static buffer already taken", -1);
    };

    if txn.set_amount(1).is_err() {
        rollback!(b"emit-txn: amount setter failed", -1);
    }
    txn.set_destination(&dest);

    let len = match txn.prepare_for_emit() {
        Ok(n) => n,
        Err(_) => rollback!(b"emit-txn: prepare_for_emit failed", -1),
    };

    let tx_blob = match txn.bytes().get(..len) {
        Some(b) => b,
        None => rollback!(b"emit-txn: prepare_for_emit returned an invalid length", -1),
    };

    match emit_buf(tx_blob) {
        Ok(_hash) => accept!(b"emit-txn: emitted", 0),
        Err(_) => rollback!(b"emit-txn: emit failed", -1),
    }
}

/// Callback invoked when the emitted transaction settles. Always accepts.
#[unsafe(no_mangle)]
pub extern "C" fn cbak(_reserved: u32) -> i64 {
    accept!()
}
