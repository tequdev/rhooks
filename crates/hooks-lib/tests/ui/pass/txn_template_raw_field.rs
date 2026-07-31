//! `txn_template!` with raw `u32` field codes — the supported form, and the
//! counterpart to the typed-`SField` fail fixture beside it.
//!
//! The prelude is imported too, so this also pins that the two tables
//! coexist: an explicit `use` of the raw names wins over the prelude's glob
//! of the typed ones.

use hooks_lib::prelude::*;
use hooks_lib::raw::sfcodes::{
    sfAccount, sfFee, sfFirstLedgerSequence, sfFlags, sfLastLedgerSequence, sfSequence,
    sfSigningPubKey,
};
use hooks_lib::txn_template;

txn_template! {
    /// A minimal template built from raw field codes.
    struct Payment {
        transaction_type = ttPAYMENT,
        flags: u32_field(sfFlags) = 0,
        sequence: u32_field(sfSequence) = 0,
        first_ledger_sequence: u32_field(sfFirstLedgerSequence) = 0,
        last_ledger_sequence: u32_field(sfLastLedgerSequence) = 0,
        fee: native_amount(sfFee) = 0,
        signing_pub_key: empty_vl(sfSigningPubKey),
        account: account_id(sfAccount),
        emit_details: emit_details,
    }
}

fn main() {
    let txn = Payment::new();
    let _ = &txn;

    // The prelude is imported above and its typed constants coexist with the
    // explicitly-imported raw ones: same names, explicit `use` wins.
    let _: SField<u32> = hooks_lib::sfield::sfSequence;
    assert_eq!(hooks_lib::sfield::sfSequence.code(), sfSequence);
}
