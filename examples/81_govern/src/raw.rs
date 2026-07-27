//! Thin `unsafe` wrappers over exactly the `hooks_core` (raw Hook API)
//! functions this crate calls, bypassing `hooks_lib::api`'s `Result<_,
//! HookError>` decoding. See `crate`'s module doc comment ("Toolchain
//! limitation") for why — the same reason documented at length in
//! `examples/80_reward/src/raw.rs`, discovered while porting that hook
//! first. Every function here returns the host's raw `i64` (negative = a
//! Hook API error code), matching govern.c's own calling convention and
//! its own (often unchecked) per-call-site error handling.

use hooks_lib::raw as hooks_core;
use hooks_lib::types::{ACC_ID_LEN, AccountId};

/// `etxn_reserve(count)`.
pub fn etxn_reserve(count: u32) -> i64 {
    unsafe { hooks_core::etxn_reserve(count) }
}

/// `otxn_field(write_ptr, write_len, field_id)`, into `out`.
pub fn otxn_field(out: &mut [u8], field_id: u32) -> i64 {
    unsafe { hooks_core::otxn_field(out.as_mut_ptr() as u32, out.len() as u32, field_id) }
}

/// `otxn_param(write_ptr, write_len, read_ptr, read_len)`, into `out`.
pub fn otxn_param(out: &mut [u8], name: &[u8]) -> i64 {
    unsafe {
        hooks_core::otxn_param(
            out.as_mut_ptr() as u32,
            out.len() as u32,
            name.as_ptr() as u32,
            name.len() as u32,
        )
    }
}

/// `hook_account(write_ptr, write_len)`, into `out`.
pub fn hook_account(out: &mut [u8]) -> i64 {
    unsafe { hooks_core::hook_account(out.as_mut_ptr() as u32, out.len() as u32) }
}

/// `hook_param(write_ptr, write_len, read_ptr, read_len)`, into `out`.
pub fn hook_param(out: &mut [u8], name: &[u8]) -> i64 {
    unsafe {
        hooks_core::hook_param(
            out.as_mut_ptr() as u32,
            out.len() as u32,
            name.as_ptr() as u32,
            name.len() as u32,
        )
    }
}

/// `state(write_ptr, write_len, kread_ptr, kread_len)`, into `out`.
pub fn state(out: &mut [u8], key: &[u8]) -> i64 {
    unsafe {
        hooks_core::state(
            out.as_mut_ptr() as u32,
            out.len() as u32,
            key.as_ptr() as u32,
            key.len() as u32,
        )
    }
}

/// `state(0, 0, kread_ptr, kread_len)` — as-int64 mode.
pub fn state_i64(key: &[u8]) -> i64 {
    unsafe { hooks_core::state(0, 0, key.as_ptr() as u32, key.len() as u32) }
}

/// `state_set(read_ptr, read_len, kread_ptr, kread_len)`.
pub fn state_set(data: &[u8], key: &[u8]) -> i64 {
    unsafe {
        hooks_core::state_set(
            data.as_ptr() as u32,
            data.len() as u32,
            key.as_ptr() as u32,
            key.len() as u32,
        )
    }
}

/// `state_set(0, 0, kread_ptr, kread_len)` — deletes the entry at `key`.
pub fn state_delete(key: &[u8]) -> i64 {
    unsafe { hooks_core::state_set(0, 0, key.as_ptr() as u32, key.len() as u32) }
}

/// `slot_set(read_ptr, read_len, slot_into)`.
pub fn slot_set(data: &[u8], slot_into: u32) -> i64 {
    unsafe { hooks_core::slot_set(data.as_ptr() as u32, data.len() as u32, slot_into) }
}

/// `slot_subfield(parent_slot, field_id, new_slot)`.
pub fn slot_subfield(parent_slot: u32, field_id: u32, new_slot: u32) -> i64 {
    unsafe { hooks_core::slot_subfield(parent_slot, field_id, new_slot) }
}

/// `slot_subarray(parent_slot, array_id, new_slot)`.
pub fn slot_subarray(parent_slot: u32, array_id: u32, new_slot: u32) -> i64 {
    unsafe { hooks_core::slot_subarray(parent_slot, array_id, new_slot) }
}

/// `slot(write_ptr, write_len, slot_no)`, into `out`.
pub fn slot(out: &mut [u8], slot_no: u32) -> i64 {
    unsafe { hooks_core::slot(out.as_mut_ptr() as u32, out.len() as u32, slot_no) }
}

/// `util_keylet(write_ptr, write_len, keylet_type, a, b, c, d, e, f)`.
#[allow(clippy::too_many_arguments)]
pub fn util_keylet(
    out: &mut [u8],
    keylet_type: u32,
    a: u32,
    b: u32,
    c: u32,
    d: u32,
    e: u32,
    f: u32,
) -> i64 {
    unsafe {
        hooks_core::util_keylet(
            out.as_mut_ptr() as u32,
            out.len() as u32,
            keylet_type,
            a,
            b,
            c,
            d,
            e,
            f,
        )
    }
}

/// `util_keylet(write_ptr, write_len, KEYLET_HOOK, account_ptr,
/// account_len, 0, 0, 0, 0)`.
pub fn hook_keylet(out: &mut [u8], account: &AccountId) -> i64 {
    util_keylet(
        out,
        hooks_core::KEYLET_HOOK,
        account.as_ptr() as u32,
        ACC_ID_LEN as u32,
        0,
        0,
        0,
        0,
    )
}

/// `util_keylet(write_ptr, write_len, KEYLET_HOOK_DEFINITION, hash_ptr,
/// 32, 0, 0, 0, 0)`.
pub fn hook_definition_keylet(out: &mut [u8], hash: &[u8; 32]) -> i64 {
    util_keylet(
        out,
        hooks_core::KEYLET_HOOK_DEFINITION,
        hash.as_ptr() as u32,
        32,
        0,
        0,
        0,
        0,
    )
}

/// `etxn_details(write_ptr, write_len)`, into `out`.
pub fn etxn_details(out: &mut [u8]) -> i64 {
    unsafe { hooks_core::etxn_details(out.as_mut_ptr() as u32, out.len() as u32) }
}

/// `etxn_fee_base(read_ptr, read_len)`.
pub fn etxn_fee_base(tx_blob: &[u8]) -> i64 {
    unsafe { hooks_core::etxn_fee_base(tx_blob.as_ptr() as u32, tx_blob.len() as u32) }
}

/// `emit(write_ptr, write_len, read_ptr, read_len)`, into `out_hash`.
pub fn emit(out_hash: &mut [u8], tx_blob: &[u8]) -> i64 {
    unsafe {
        hooks_core::emit(
            out_hash.as_mut_ptr() as u32,
            out_hash.len() as u32,
            tx_blob.as_ptr() as u32,
            tx_blob.len() as u32,
        )
    }
}

/// `ledger_seq()`.
pub fn ledger_seq() -> u32 {
    unsafe { hooks_core::ledger_seq() as u32 }
}
