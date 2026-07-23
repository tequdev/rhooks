//! `emit-txn` — reserves an emission slot and emits a 1-drop Payment back
//! to the account that sent the originating transaction, using
//! `prepare()` to fill in host-computed system fields. Also exports a
//! `cbak` callback for when the emitted transaction settles.
//!
//! Build: `hooks-build build --manifest-path examples/emit-txn/Cargo.toml`
//!
//! ## A note on `prepare()`
//!
//! `prepare()` is a HooksUpdate2+ Hook API function documented (in the
//! reference material available while writing this example) only as
//! "auto-fill system fields" for an emitted-transaction template; its
//! exact substitution contract is not further specified there. This
//! example follows that one documented usage pattern: build a minimal
//! pre-image `STObject` containing only the fields a Hook author actually
//! chooses (`TransactionType`, `Flags`, `Amount`, `Account`,
//! `Destination`, in ascending canonical field order) and let `prepare()`
//! compute and insert the remaining system fields (`Sequence`, `Fee`,
//! `SigningPubKey`, `EmitDetails`, `LastLedgerSequence`). This has not
//! been exercised against a live xahaud node from this repository —
//! `hooks-build` validates wasm module shape only, not the wire-level
//! correctness of an emitted transaction blob.

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, rollback};

/// Size of the minimal pre-image `STObject` passed to `prepare()`:
/// `TransactionType` (3 bytes) + `Flags` (5 bytes) + `Amount` (9 bytes) +
/// `Account` (2 + 20 bytes) + `Destination` (2 + 20 bytes) = 61 bytes, all
/// fields in ascending canonical (type, field) order.
const TX_PRE_LEN: usize = 61;

/// Output buffer size for `prepare()`. Comfortably covers the pre-image
/// plus the system fields it is expected to insert (`Sequence`, `Fee`,
/// `SigningPubKey`, `EmitDetails`, `LastLedgerSequence`).
const PREPARE_BUF_LEN: usize = 320;

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

    let account = match hook_account() {
        Ok(a) => a,
        Err(_) => rollback!(b"emit-txn: hook_account failed", -1),
    };

    // Minimal pre-image STObject, ascending canonical field order:
    // TransactionType(1,2), Flags(2,2), Amount(6,1), Account(8,1),
    // Destination(8,3). Value bytes that are zero (TransactionType =
    // ttPAYMENT = 0, Flags = 0) rely on the array's zero-initialization.
    let mut pre: [u8; TX_PRE_LEN] = [
        0x12, 0x00, 0x00, // TransactionType = ttPAYMENT (0)
        0x22, 0x00, 0x00, 0x00, 0x00, // Flags = 0
        0x61, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, // Amount: 1 drop, native
        0x81, 0x14, // Account: header + 20-byte length
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // Account placeholder
        0x83, 0x14, // Destination: header + 20-byte length
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // Destination placeholder
    ];
    // Account occupies bytes [19, 39); Destination occupies bytes [41, 61)
    // (see the layout comment above — both are literal, fixed offsets).
    pre[19..39].copy_from_slice(&account);
    pre[41..61].copy_from_slice(&dest);

    let mut prepared = [0u8; PREPARE_BUF_LEN];
    let plen = match prepare(&mut prepared, &pre) {
        Ok(n) => n,
        Err(_) => rollback!(b"emit-txn: prepare failed", -1),
    };

    let tx_blob = match prepared.get(..plen) {
        Some(b) => b,
        None => rollback!(b"emit-txn: prepare returned an invalid length", -1),
    };

    match emit(tx_blob) {
        Ok(_hash) => accept!(b"emit-txn: emitted", 0),
        Err(_) => rollback!(b"emit-txn: emit failed", -1),
    }
}

/// Callback invoked when the emitted transaction settles. Always accepts.
#[unsafe(no_mangle)]
pub extern "C" fn cbak(_reserved: u32) -> i64 {
    accept!()
}
