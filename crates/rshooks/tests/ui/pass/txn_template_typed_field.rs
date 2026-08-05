//! `txn_template!` takes the **typed** prelude constants.
//!
//! The macro's expansion wraps every field code as `($sfcode).code()` — a
//! `const fn` call, which is legal in the const context its field table is
//! built in, where `Into` would not be. So a template reads with the same
//! `sfXxx` names as everything else in a hook, with no `raw::sfcodes` detour
//! at the call site.
//!
//! (This replaced a raw-`u32`-only grammar. The fail fixture beside this one
//! pins the consequence: a bare `u32` expression no longer works.)

use rshooks::prelude::*;
use rshooks::txn_template;

txn_template! {
    /// A minimal template built from the prelude's typed field constants.
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

    // `.code()` is still the bridge for a const context that needs the raw
    // `u32` itself — a hand-rolled header table, say.
    const FLAGS: u32 = sfFlags.code();
    assert_eq!(FLAGS, rshooks::raw::sfcodes::sfFlags);
}
