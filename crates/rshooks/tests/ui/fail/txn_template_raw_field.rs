//! The consequence of `txn_template!` taking typed constants: a bare `u32`
//! field code no longer works, because the expansion calls `.code()` on
//! whatever it is given.
//!
//! Raw codes are still reachable everywhere else — every runtime field-code
//! parameter takes `impl Into<u32>`, and `rshooks::raw::sfcodes` is still
//! exported — but a template's field list is typed now.

use rshooks::prelude::*;
use rshooks::txn_template;

txn_template! {
    /// A template whose field codes are raw `u32`s — rejected.
    struct Payment {
        transaction_type = ttPAYMENT,
        flags: u32_field(rshooks::raw::sfcodes::sfFlags) = 0,
        sequence: u32_field(rshooks::raw::sfcodes::sfSequence) = 0,
        first_ledger_sequence: u32_field(rshooks::raw::sfcodes::sfFirstLedgerSequence) = 0,
        last_ledger_sequence: u32_field(rshooks::raw::sfcodes::sfLastLedgerSequence) = 0,
        fee: native_amount(rshooks::raw::sfcodes::sfFee) = 0,
        signing_pub_key: empty_vl(rshooks::raw::sfcodes::sfSigningPubKey),
        account: account_id(rshooks::raw::sfcodes::sfAccount),
        emit_details: emit_details,
    }
}

fn main() {}
