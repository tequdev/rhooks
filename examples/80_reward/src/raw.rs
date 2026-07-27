//! Thin `unsafe` wrappers over exactly the `hooks_core` (raw Hook API)
//! functions this crate calls, bypassing `hooks_lib::api`'s `Result<_,
//! HookError>` decoding. See `crate`'s module doc comment ("Toolchain
//! limitation") for why this crate needs this instead of the ordinary
//! `hooks_lib::api`/`XFL` wrappers every other example in this repo uses.
//!
//! Every function here returns the host's raw `i64` (negative = a Hook
//! API error code) or writes into a caller buffer and returns the raw
//! byte count/status the same way — i.e. exactly reward.c's own C calling
//! convention, not reinterpreted at all. Callers range-check the
//! returned value themselves, matching reward.c's own per-call-site
//! checks (or lack thereof) — see each call site in `lib.rs` for the
//! matching reward.c line.

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

/// `hook_account(write_ptr, write_len)`, into `out`.
pub fn hook_account(out: &mut [u8]) -> i64 {
    unsafe { hooks_core::hook_account(out.as_mut_ptr() as u32, out.len() as u32) }
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

/// Reads `key` as an 8-byte little-endian XFL (matching
/// `hooks_lib::api::state::state_xfl`'s own encoding convention), falling
/// back to `default` on any failure — including a missing entry —
/// matching reward.c's `state(&xfl_rr, 8, "RR", 2)`, whose failure leaves
/// the pre-set default untouched.
pub fn state_xfl_or(key: &[u8], default: i64) -> i64 {
    let mut buf = [0u8; 8];
    if state(&mut buf, key) == 8 {
        i64::from_le_bytes(buf)
    } else {
        default
    }
}

/// `slot(write_ptr, write_len, slot_no)`, into `out`.
pub fn slot(out: &mut [u8], slot_no: u32) -> i64 {
    unsafe { hooks_core::slot(out.as_mut_ptr() as u32, out.len() as u32, slot_no) }
}

/// `slot(0, 0, slot_no)` — the Hook API's "as-int64" mode: the host packs
/// the slot's raw bytes directly into the returned `i64` instead of
/// writing to a buffer (only valid for slot data of at most 8 bytes).
pub fn slot_i64(slot_no: u32) -> i64 {
    unsafe { hooks_core::slot(0, 0, slot_no) }
}

/// `slot_count(slot_no)`.
pub fn slot_count(slot_no: u32) -> i64 {
    unsafe { hooks_core::slot_count(slot_no) }
}

/// `slot_set(read_ptr, read_len, slot_into)`.
pub fn slot_set(data: &[u8], slot_into: u32) -> i64 {
    unsafe { hooks_core::slot_set(data.as_ptr() as u32, data.len() as u32, slot_into) }
}

/// `slot_subfield(parent_slot, field_id, new_slot)`.
pub fn slot_subfield(parent_slot: u32, field_id: u32, new_slot: u32) -> i64 {
    unsafe { hooks_core::slot_subfield(parent_slot, field_id, new_slot) }
}

/// `slot_float(slot_no)`.
pub fn slot_float(slot_no: u32) -> i64 {
    unsafe { hooks_core::slot_float(slot_no) }
}

/// `otxn_slot(slot_into)`.
pub fn otxn_slot(slot_into: u32) -> i64 {
    unsafe { hooks_core::otxn_slot(slot_into) }
}

/// `util_keylet(write_ptr, write_len, KEYLET_ACCOUNT, account_ptr,
/// account_len, 0, 0, 0, 0)` — reward.c's `util_keylet(SBUF(kl),
/// KEYLET_ACCOUNT, SBUF(otxn_acc), 0,0,0,0)`.
pub fn account_keylet(out: &mut [u8], account: &AccountId) -> i64 {
    unsafe {
        hooks_core::util_keylet(
            out.as_mut_ptr() as u32,
            out.len() as u32,
            hooks_core::KEYLET_ACCOUNT,
            account.as_ptr() as u32,
            ACC_ID_LEN as u32,
            0,
            0,
            0,
            0,
        )
    }
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

/// `float_set(exponent, mantissa)`.
pub fn float_set(exponent: i32, mantissa: i64) -> i64 {
    unsafe { hooks_core::float_set(exponent, mantissa) }
}

/// `float_divide(a, b)`.
pub fn float_divide(a: i64, b: i64) -> i64 {
    unsafe { hooks_core::float_divide(a, b) }
}

/// `float_multiply(a, b)`.
pub fn float_multiply(a: i64, b: i64) -> i64 {
    unsafe { hooks_core::float_multiply(a, b) }
}

/// `float_int(x, decimal_places, abs)`.
pub fn float_int(x: i64, decimal_places: u32, abs: u32) -> i64 {
    unsafe { hooks_core::float_int(x, decimal_places, abs) }
}

/// `float_sign(x)`.
pub fn float_sign(x: i64) -> i64 {
    unsafe { hooks_core::float_sign(x) }
}

/// `float_one()`.
pub fn float_one() -> i64 {
    unsafe { hooks_core::float_one() }
}

/// `float_compare(a, b, mode)`.
pub fn float_compare(a: i64, b: i64, mode: u32) -> i64 {
    unsafe { hooks_core::float_compare(a, b, mode) }
}
