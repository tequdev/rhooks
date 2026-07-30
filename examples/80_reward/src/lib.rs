//! `reward` — a behavior-equivalent Rust port of xahaud's genesis
//! `RewardHook` (`hook/genesis/reward.c`).
//!
//! `ttCLAIM_REWARD` transactions land on an account that has this hook
//! installed. This hook computes the account's XAH-hours-accrued reward
//! since its last claim, converts it through the governance-set reward
//! rate, and emits a `GenesisMint` transaction (see [`mint_txn`]) crediting
//! the claimant and every active-validator L1 governance seat. See the
//! README for the full behavior-equivalence table (input case ->
//! output/state effect, matched against reward.c's branches) and this
//! crate's differences from reward.c.
//!
//! Every Hook API call in this file goes through `hooks_lib`'s ordinary
//! `Result`-based wrappers (`otxn_field_exact`, `hook_account_buf`,
//! `slot_subfield`, `slot_u64`, `util_keylet`, `emit_buf`, ...) — the
//! natural one for each call site (an `_exact`/`_buf` convenience where
//! one exists and fits, the plain wrapper otherwise) — **except** the
//! small [`raw`] module, scoped to exactly reward.c's `float_*`/
//! `slot_float` calls. See [`raw`]'s doc comment for why XFL specifically
//! is the one place this crate steps around `hooks_lib::xfl::XFL`.
//!
//! Build: `hooks-build build --manifest-path examples/80_reward/Cargo.toml`

#![no_std]

mod message;
mod mint_txn;
mod raw;

use hooks_lib::prelude::*;
use hooks_lib::static_cell::HookStatic;
use hooks_lib::{accept, guard, hook, hook_errors, hook_state, rollback};
use mint_txn::{L1_SEATS, MintTxn};

/// `DEFAULT_REWARD_RATE` in reward.c: `0.00333333333` as raw XFL bits, used
/// only if hook state has no `"RR"` entry (should not happen once governed,
/// but reward.c falls back gracefully rather than failing outright).
const DEFAULT_REWARD_RATE_BITS: i64 = 6_038_156_834_009_797_973;

/// `DEFAULT_REWARD_DELAY` in reward.c: 2,600,000 seconds as raw XFL bits.
const DEFAULT_REWARD_DELAY_BITS: i64 = 6_199_553_087_261_802_496;

// `"RR"`/`"RD"` — the same 2-byte real-length keys govern.c/govern.rs write
// under (see `examples/81_govern/src/keys.rs::REWARD_RATE`/`REWARD_DELAY`)
// — declared via `hook_state!`'s Form 1 (a fully fixed key), read through
// the typed layer below. Safe under the 32-level guard-checker limit
// because `crate::state::decode_read` compares the *raw* host-call code
// against `hooks_core::DOESNT_EXIST` before ever constructing a
// `HookError` (see DESIGN.md §5.1) — measured nesting depth here is 24/32.
// `XFL`'s typed-layer decode is little-endian raw-bits, byte-for-byte
// identical to the raw `state_xfl` this replaces (both require exactly 8
// stored bytes, which every writer of these keys always provides).
hook_state!(RewardRateKey = b"RR" => XFL);
hook_state!(RewardDelayKey = b"RD" => XFL);

/// `MAXUNL` in reward.c: `UNLReport`'s `ActiveValidators` array is assumed
/// to hold at most this many entries.
const MAX_UNL: usize = 128;

/// Byte offset of the first validator-owner-account within each
/// `ActiveValidators` entry, matching reward.c's `av_array + 39`
/// (`UNLReport`'s per-entry layout: a `PublicKey` field header/payload
/// precede the 20-byte account).
const AV_FIRST_KEY_OFFSET: usize = 39;

/// Byte stride between consecutive `ActiveValidators` entries, matching
/// reward.c's `av_upto += 60U`.
const AV_ENTRY_STRIDE: usize = 60;

/// Worst-case serialized size of the `UNLReport`'s `ActiveValidators`
/// array, matching reward.c's `av_array[(60 * MAXUNL) + 4]`.
const AV_ARRAY_LEN: usize = AV_ENTRY_STRIDE * MAX_UNL + 4;

/// The `UNLReport` ledger object's fixed Keylet — a protocol-level
/// constant (independent of any account), transcribed verbatim from
/// reward.c's `unlreport_keylet`.
const UNLREPORT_KEYLET: [u8; 34] = [
    0x00, 0x52, 0x61, 0xE3, 0x2E, 0x7A, 0x24, 0xA2, 0x38, 0xF1, 0xC6, 0x19, 0xD5, 0xF9, 0xDD, 0xCC,
    0x41, 0xA9, 0x4B, 0x33, 0xB6, 0x6C, 0x01, 0x63, 0xF7, 0xEF, 0xCC, 0x8A, 0x19, 0xC9, 0xFD, 0x6F,
    0x28, 0xDC,
];

hook_errors! {
    /// `reward`'s rollback reasons. reward.c itself rolls back with
    /// `__LINE__` as the code (meaningful only for its own source, not a
    /// stable protocol value) — this hook instead uses a small stable
    /// enum, per this repo's convention (see `examples/04_errors`). Only
    /// `accept`/`rollback` outcomes and messages are behavior-equivalence
    /// targets (see the README's differences table), not the numeric
    /// code.
    pub enum RewardError {
        /// Governance has disabled rewards (`RR`/`RD` state <= 0).
        RewardsDisabled = -101,
        /// `RR`/`RD` fail reward.c's sanity bounds (rate in `(0, 1]`,
        /// delay `>= 1` second).
        MisconfiguredReward = -102,
        /// An invariant reward.c enforces with `ASSERT`/an unchecked Hook
        /// API call failed — defensive; unreachable for a well-formed
        /// `ClaimReward` on an account already past its setup claim.
        AssertionFailed = -103,
        /// Building or submitting the `GenesisMint` emission failed.
        EmitFailed = -104,
    }
}

impl RewardError {
    fn rollback(self, msg: &[u8]) -> ! {
        rollback!(msg, self)
    }
}

/// The `GenesisMint` transaction under construction — large (see
/// `mint_txn::MintTxn`), so it lives in a wasm data segment/BSS via
/// [`HookStatic`] rather than as a stack local (see `examples/README.md`,
/// "Statics for templates and large buffers").
static MINT_TXN: HookStatic<MintTxn> = HookStatic::new(MintTxn::new());

/// `UNLReport.ActiveValidators`'s worst-case serialized bytes — also a
/// `HookStatic` for the same reason.
static AV_BUF: HookStatic<[u8; AV_ARRAY_LEN]> = HookStatic::new([0u8; AV_ARRAY_LEN]);

/// Scratch space for the sender's AccountRoot Keylet (34 bytes). A
/// `HookStatic` rather than a stack local for the same reason
/// [`MINT_TXN`]/[`AV_BUF`] are: a 34-byte-and-up zero-initialized stack
/// array is exactly the size `wasm32v1-none`'s codegen can start
/// lowering to an unguarded `memset`-style loop at some optimization
/// levels (see `examples/README.md`'s "Statics for templates and large
/// buffers") — unrelated to which API wraps `util_keylet` itself.
static ACCOUNT_KEYLET: HookStatic<[u8; KEYLET_LEN]> = HookStatic::new([0u8; KEYLET_LEN]);

#[hook]
fn my_hook() -> i64 {
    if etxn_reserve(1).is_err() {
        RewardError::EmitFailed.rollback(b"reward: etxn_reserve failed");
    }

    // Only TxType::ClaimReward is of interest; everything else (including
    // this hook's own emitted GenesisMint settling, and any other incoming
    // transaction type on an account HookOn'd broadly) passes through.
    if otxn_type() != TxType::ClaimReward {
        accept!(b"Reward: Passing non-claim txn", 0);
    }

    let sender: AccountId = match otxn_field_exact(sfAccount) {
        Ok(a) => a,
        Err(_) => RewardError::AssertionFailed.rollback(b"reward: could not read otxn Account"),
    };
    let hook_acc: AccountId = match hook_account_buf() {
        Ok(a) => a,
        Err(_) => RewardError::AssertionFailed.rollback(b"reward: could not read hook_account"),
    };
    // The hook's own emitted ClaimReward-adjacent traffic (there is none
    // today, but reward.c guards this unconditionally) passes through.
    if buf_eq_20(&sender, &hook_acc) {
        accept!(b"Reward: Passing outgoing txn", 0);
    }

    let rr = state_get_typed(&RewardRateKey)
        .unwrap_or(None)
        .unwrap_or(XFL::from_raw_bits(DEFAULT_REWARD_RATE_BITS));
    let rd = state_get_typed(&RewardDelayKey)
        .unwrap_or(None)
        .unwrap_or(XFL::from_raw_bits(DEFAULT_REWARD_DELAY_BITS));
    let rewards_disabled = (rr.raw_bits() <= 0) | (rd.raw_bits() <= 0);
    if rewards_disabled {
        RewardError::RewardsDisabled.rollback(b"Reward: Rewards are disabled by governance.");
    }

    // reward.c treats any of these four checks failing (`required_delay <
    // 0`, `rr` negative, `rr > 1`, `rd < 1`) as "misconfigured," rolling
    // back with the same message — and, since a `float_*` host call
    // returning a raw negative *error* code would *also* read as "< 0" or
    // "!= 0" in reward.c's C, an XFL operation failing outright leads to
    // exactly the same rollback there too. See [`raw`]'s doc comment for
    // why these specific calls (and only these) bypass `XFL`.
    const MISCONFIGURED_MSG: &[u8] =
        b"Reward: Rewards incorrectly configured by governance or unrecoverable error.";
    let required_delay = raw::float_int(rd.raw_bits(), 0, 0);
    let misconfigured = (required_delay < 0)
        | (raw::float_sign(rr.raw_bits()) != 0)
        | (raw::float_compare(rr.raw_bits(), raw::float_one(), COMPARE_GREATER) != 0)
        | (raw::float_compare(rd.raw_bits(), raw::float_one(), COMPARE_LESS) != 0);
    if misconfigured {
        RewardError::MisconfiguredReward.rollback(MISCONFIGURED_MSG);
    }

    // Slot the sender's AccountRoot; `RewardAccumulator` only exists once
    // a prior ClaimReward has already run the protocol-level reward setup
    // on this account. `util_keylet`, not `util_keylet_buf`: the output
    // still needs to land in the `HookStatic` scratch buffer above, not a
    // fresh stack-local `Keylet` (see `ACCOUNT_KEYLET`'s doc comment).
    let Some(kl_buf) = ACCOUNT_KEYLET.take() else {
        RewardError::AssertionFailed.rollback(b"reward: account keylet buffer already taken");
    };
    let kl_len = match util_keylet(
        kl_buf,
        KEYLET_ACCOUNT,
        sender.as_ptr() as u32,
        ACC_ID_LEN as u32,
        0,
        0,
        0,
        0,
    ) {
        Ok(n) => n,
        Err(_) => RewardError::AssertionFailed.rollback(b"reward: could not build account keylet"),
    };
    let Some(kl) = kl_buf.get(..kl_len) else {
        RewardError::AssertionFailed.rollback(b"reward: could not build account keylet");
    };
    if slot_set(kl, 1).is_err() {
        RewardError::AssertionFailed.rollback(b"reward: could not slot sender account");
    }
    if slot_subfield(1, sfRewardAccumulator, 2) != Ok(2) {
        accept!(b"Reward: Passing reward setup txn", 0);
    }
    let has_first = slot_subfield(1, sfRewardLgrFirst, 3) == Ok(3);
    let has_last = slot_subfield(1, sfRewardLgrLast, 4) == Ok(4);
    let has_balance = slot_subfield(1, sfBalance, 5) == Ok(5);
    let has_time = slot_subfield(1, sfRewardTime, 6) == Ok(6);
    if !(has_first & has_last & has_balance & has_time) {
        RewardError::AssertionFailed.rollback(b"reward: missing reward accounting fields");
    }

    let last_claim_time = slot_u64(6).unwrap_or(0);
    let time_elapsed = ledger_last_time().wrapping_sub(last_claim_time);
    let required_delay = required_delay as u64;
    if time_elapsed < required_delay {
        let remaining = required_delay.wrapping_sub(time_elapsed);
        rollback!(&message::wait_message(remaining), 0);
    }

    let accumulator = slot_u64(2).unwrap_or(0) as i64;
    let first = slot_u64(3).unwrap_or(0) as i64;
    let last = slot_u64(4).unwrap_or(0) as i64;
    let raw_balance = slot_u64(5).unwrap_or(0);

    if (first <= 0) | (last <= 0) {
        RewardError::AssertionFailed.rollback(b"Reward: Assertion failure.");
    }

    let cur = i64::from(ledger_seq());
    let elapsed = cur.wrapping_sub(first);
    if elapsed <= 0 {
        RewardError::AssertionFailed.rollback(b"Reward: Assertion failure.");
    }
    let elapsed_since_last = cur.wrapping_sub(last);

    // `Balance`'s raw as-int64 form keeps the native-amount control bits
    // set; mask them off, then convert drops -> whole XAH (reward.c's
    // `bal &= ~0xE000000000000000ULL; bal /= 1000000LL`).
    let bal = (raw_balance & !0xE000_0000_0000_0000u64).wrapping_div(1_000_000);

    let accumulator = if bal > 0 && elapsed_since_last > 0 {
        accumulator.wrapping_add((bal as i64).wrapping_mul(elapsed_since_last))
    } else {
        accumulator
    };

    // reward.c's own reward-rate math `ASSERT`s only on the two
    // `float_set` results — the subsequent `float_divide`/
    // `float_multiply`/`float_int` calls are never separately checked in
    // reward.c either, so a host-level failure there flows through as
    // whatever raw value the failed call returns, same as reward.c's own
    // `uint64_t reward_drops = float_int(...)` (a negative `float_int`
    // result becomes a huge `u64` via the same two's-complement
    // reinterpretation C's implicit conversion performs). See [`raw`]'s
    // doc comment for why this arithmetic chain is exactly where `XFL`'s
    // validation would both cost the most and matter the least.
    let xfl_accum = raw::float_set(0, accumulator);
    if xfl_accum <= 0 {
        RewardError::AssertionFailed.rollback(b"Reward: Assertion failure.");
    }
    let xfl_elapsed = raw::float_set(0, elapsed);
    if xfl_elapsed <= 0 {
        RewardError::AssertionFailed.rollback(b"Reward: Assertion failure.");
    }
    let per_ledger = raw::float_divide(xfl_accum, xfl_elapsed);
    let xfl_reward = raw::float_multiply(rr.raw_bits(), per_ledger);
    let base_reward_drops = raw::float_int(xfl_reward, 6, 1) as u64;
    // `L1_SEATS` (20) is a nonzero compile-time constant; `wrapping_div`'s
    // only panicking case is a zero divisor, which this can never be —
    // clippy just can't see across the `mint_txn::L1_SEATS` import to
    // prove it.
    #[allow(clippy::arithmetic_side_effects)]
    let l1_drops = base_reward_drops.wrapping_div(L1_SEATS as u64);

    let otxn_slot_num = otxn_slot(10);
    if otxn_slot_num != Ok(10) {
        RewardError::AssertionFailed.rollback(b"reward: could not slot otxn");
    }
    if slot_subfield(10, sfFee, 11) != Ok(11) {
        RewardError::AssertionFailed.rollback(b"reward: could not slot otxn Fee");
    }
    // reward.c: `int64_t xfl_fee = slot_float(11);` — also unchecked; see
    // [`raw`]'s doc comment for why this reads the slot as a raw XFL bit
    // pattern instead of through `XFL::from_slot`.
    let xfl_fee = raw::slot_float(11);
    let rewardee_drops = if xfl_fee > 0 {
        let fee_drops = raw::float_int(xfl_fee, 6, 1);
        base_reward_drops.wrapping_add(fee_drops as u64)
    } else {
        base_reward_drops
    };

    let Some(txn) = MINT_TXN.take() else {
        RewardError::EmitFailed.rollback(b"reward: mint txn buffer already taken");
    };
    // `MintTxn`'s methods roll back internally on the (unreachable in
    // practice) failure paths instead of returning `Result` — see
    // `mint_txn`'s module doc comment for why.
    txn.start();
    txn.write_emit_details();
    txn.push_entry(rewardee_drops, &sender);

    push_l1_seat_entries(txn, l1_drops);

    let bytes = txn.finish(ledger_seq());

    match emit_buf(bytes) {
        Ok(_hash) => accept!(b"Reward: Emitted reward txn successfully.", 0),
        Err(_) => RewardError::EmitFailed.rollback(b"Reward: Emit loopback failed."),
    }
}

/// Appends one `GenesisMint` entry per L1 seat currently occupied by an
/// active validator (reward.c's `can_reward`/seat-iteration loops). Any
/// failure here is non-fatal to the overall claim (matching reward.c,
/// which treats the whole `UNLReport`-driven L1 distribution as
/// best-effort and always proceeds to emit at least the rewardee entry).
fn push_l1_seat_entries(txn: &mut MintTxn, l1_drops: u64) {
    if slot_set(&UNLREPORT_KEYLET, 1) != Ok(1) {
        return;
    }
    if slot_subfield(1, sfActiveValidators, 1) != Ok(1) {
        return;
    }
    let Some(av_buf) = AV_BUF.take() else {
        return;
    };
    let Ok(read_len) = slot(av_buf, 1) else {
        return;
    };
    if read_len == 0 {
        return;
    }
    let Ok(count) = slot_count(1) else {
        return;
    };
    let av_size = (count as usize).min(MAX_UNL);

    // Flattened with `let-else` + `continue` throughout (rather than
    // nested `if let`) so each loop body stays a single level deep — the
    // Hook API's static guard checker rejects wasm control flow nested
    // past 32 levels, and a chain of nested `if let`s here compounds with
    // this function's own inlining into `my_hook()`.
    let mut can_reward = [false; L1_SEATS];
    let mut i = 0usize;
    while i < av_size {
        guard!(MAX_UNL as u32);
        let offset = AV_FIRST_KEY_OFFSET.wrapping_add(AV_ENTRY_STRIDE.wrapping_mul(i));
        i = i.wrapping_add(1);

        let Some(key) = av_buf.get(offset..offset.wrapping_add(ACC_ID_LEN)) else {
            continue;
        };
        let mut seat = [0u8; 1];
        if state(&mut seat, key) != Ok(1) {
            continue;
        }
        let Some(&seat) = seat.first() else {
            continue;
        };
        // C's off-by-one (`seat > L1SEATS` rather than `>=`, so a stored
        // seat byte of exactly 20 is let through) is preserved here too,
        // but `can_reward.get_mut` turns the resulting out-of-range index
        // into a safe no-op instead of C's out-of-bounds array write — see
        // the README's differences table. `seat` values only ever come
        // from govern.rs/govern.c, which never assign 20, so this is
        // unreachable in practice.
        if seat as usize > L1_SEATS {
            continue;
        }
        let Some(slot) = can_reward.get_mut(seat as usize) else {
            continue;
        };
        *slot = true;
    }

    // A manual `while`, not `for seat in 0u8..(L1_SEATS as u8)`: a `for`
    // loop's `Iterator::next()` bookkeeping compiles to real instructions
    // *before* the loop body, which pushes `guard!`'s `_g` call out of
    // the very first position in the compiled `loop` — and the Hook
    // API's guard checker specifically requires `i32.const; i32.const;
    // call $_g` to be the first three instructions after `loop` (see
    // `examples/06_guard-patterns`'s own `while`-only convention).
    let mut seat = 0u8;
    while seat < L1_SEATS as u8 {
        guard!(L1_SEATS as u32);
        let this_seat = seat;
        seat = seat.wrapping_add(1);
        if !can_reward.get(this_seat as usize).copied().unwrap_or(false) {
            continue;
        }
        let mut destination = AccountId::zeroed();
        if state(destination.as_mut(), core::slice::from_ref(&this_seat)) != Ok(ACC_ID_LEN) {
            continue;
        }
        // At most `L1_SEATS` seat entries are ever pushed here (see
        // `mint_txn::MAX_ENTRIES`), so `push_entry` never actually hits its
        // internal overflow check in practice.
        txn.push_entry(l1_drops, &destination);
    }
}
