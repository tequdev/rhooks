//! `txn_template!` builds its field table in a **const** context, and `Into`
//! is not const — so its grammar takes raw `u32` const expressions only. A
//! typed `SField` there is a compile error; `.code()` (or the raw constant)
//! is the bridge. The pass fixture beside this one shows the working form.

use hooks_lib::prelude::*;
use hooks_lib::txn_template;

txn_template! {
    /// A template whose field codes are typed constants — rejected.
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

fn main() {}
