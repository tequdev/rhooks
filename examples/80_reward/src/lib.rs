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
//! # Toolchain limitation: raw Hook API calls in this file
//!
//! Every fallible Hook API call below goes through [`raw`]'s thin
//! `unsafe` wrappers instead of `hooks_lib::api`'s `Result<_, HookError>`
//! ones. This is a deliberate, narrow exception to the rest of this
//! repo's hooks-lib-idiomatic style (see e.g. `examples/07_xfl-math`,
//! which uses the `Result` API throughout without issue): once
//! `hooks-build`'s Guard-type pipeline inlines every function in a crate
//! into `hook()` (`docs/DESIGN.md` §6.2c), `HookError`'s ~40-variant
//! `From<i64>` decode — baked into every error path `hooks_lib::error::res`
//! funnels through, even when a caller only compares the `Result` to a
//! specific `Ok` value or discards the `Err` payload entirely — was
//! measured (via `crates/hooks-build/examples/diag.rs`, a throwaway
//! pipeline-stage dumper written for this investigation) to compile to a
//! wasm `br_table` needing roughly 40 nested `block`s per call site. This
//! hook has enough distinct fallible calls that, combined, they pushed
//! `hook()`'s block/loop/if nesting well past the vendored guard checker's
//! 32-level limit (`Guard.h`'s `NESTING_LIMIT`) even after every
//! source-level mitigation short of this one (bumping `[profile.release]`
//! `opt-level` from `"z"` to `3`, restructuring boolean chains to use
//! eager `|`/`&`, factoring checks into dedicated functions, converting
//! `MintTxn`'s internal API from `Result`-propagating to rollback-on-
//! failure — see `mint_txn`'s and `message`'s module doc comments for the
//! ones that carry their own rationale). This crate's own [`raw`] module
//! only exposes the small subset of `hooks_core`'s raw signatures reward.c
//! itself calls, each documented against the exact reward.c line(s) it
//! stands in for; every one matches reward.c's own behavior *more*
//! closely than the `Result`-wrapped equivalent, since reward.c itself
//! never separately checks most of these calls either — it reuses the
//! raw negative-i64-is-an-error convention directly in its own range
//! checks (`xfl_rr <= 0`, `required_delay < 0`, ...). See the README's
//! "Toolchain limitation: `HookError` decoding and nesting depth" section
//! for the full writeup and reproduction steps; this is flagged there as
//! a candidate `hooks-lib`/`hooks-build` fix, not something every complex
//! hook should have to work around by hand.
//!
//! Build: `hooks-build build --manifest-path examples/80_reward/Cargo.toml`

#![no_std]

mod message;
mod mint_txn;
mod raw;

use hooks_lib::prelude::*;
use hooks_lib::static_cell::HookStatic;
use hooks_lib::{accept, guard, hook, hook_errors, rollback};
use mint_txn::{L1_SEATS, MintTxn};

/// `DEFAULT_REWARD_RATE` in reward.c: `0.00333333333` as raw XFL bits, used
/// only if hook state has no `"RR"` entry (should not happen once governed,
/// but reward.c falls back gracefully rather than failing outright).
const DEFAULT_REWARD_RATE_BITS: i64 = 6_038_156_834_009_797_973;

/// `DEFAULT_REWARD_DELAY` in reward.c: 2,600,000 seconds as raw XFL bits.
const DEFAULT_REWARD_DELAY_BITS: i64 = 6_199_553_087_261_802_496;

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

/// Scratch space for [`raw::account_keylet`]'s 34-byte output. A
/// `HookStatic` for the same reason as [`MINT_TXN`]/[`AV_BUF`]: a
/// 34-byte-and-up zero-initialized stack array is exactly the size
/// `wasm32v1-none`'s codegen can start lowering to an unguarded
/// `memset`-style loop at some optimization levels (see
/// `examples/README.md`'s "Statics for templates and large buffers").
static ACCOUNT_KEYLET: HookStatic<[u8; KEYLET_LEN]> = HookStatic::new([0u8; KEYLET_LEN]);

#[hook]
fn my_hook() -> i64 {
    if raw::etxn_reserve(1) < 0 {
        RewardError::EmitFailed.rollback(b"reward: etxn_reserve failed");
    }

    // Only ttCLAIM_REWARD is of interest; everything else (including this
    // hook's own emitted GenesisMint settling, and any other incoming
    // transaction type on an account HookOn'd broadly) passes through.
    if u32::from(otxn_type()) != ttCLAIM_REWARD {
        accept!(b"Reward: Passing non-claim txn", 0);
    }

    let mut sender = AccountId::zeroed();
    if raw::otxn_field(sender.as_mut(), sfAccount) != ACC_ID_LEN as i64 {
        RewardError::AssertionFailed.rollback(b"reward: could not read otxn Account");
    }
    let mut hook_acc = AccountId::zeroed();
    if raw::hook_account(hook_acc.as_mut()) != ACC_ID_LEN as i64 {
        RewardError::AssertionFailed.rollback(b"reward: could not read hook_account");
    }
    // The hook's own emitted ClaimReward-adjacent traffic (there is none
    // today, but reward.c guards this unconditionally) passes through.
    if buf_eq_20(&sender, &hook_acc) {
        accept!(b"Reward: Passing outgoing txn", 0);
    }

    let rr = raw::state_xfl_or(b"RR", DEFAULT_REWARD_RATE_BITS);
    let rd = raw::state_xfl_or(b"RD", DEFAULT_REWARD_DELAY_BITS);
    let rewards_disabled = (rr <= 0) | (rd <= 0);
    if rewards_disabled {
        RewardError::RewardsDisabled.rollback(b"Reward: Rewards are disabled by governance.");
    }

    // reward.c treats any of these four checks failing (`required_delay <
    // 0`, `rr` negative, `rr > 1`, `rd < 1`) as "misconfigured," rolling
    // back with the same message — and, since a `float_*` host call
    // returning a raw negative *error* code would *also* read as "< 0" or
    // "!= 0" in reward.c's C, an XFL operation failing outright leads to
    // exactly the same rollback there too (see the module doc comment for
    // why every one of these is a raw call).
    const MISCONFIGURED_MSG: &[u8] =
        b"Reward: Rewards incorrectly configured by governance or unrecoverable error.";
    let required_delay = raw::float_int(rd, 0, 0);
    let misconfigured = (required_delay < 0)
        | (raw::float_sign(rr) != 0)
        | (raw::float_compare(rr, raw::float_one(), COMPARE_GREATER) != 0)
        | (raw::float_compare(rd, raw::float_one(), COMPARE_LESS) != 0);
    if misconfigured {
        RewardError::MisconfiguredReward.rollback(MISCONFIGURED_MSG);
    }

    // Slot the sender's AccountRoot; `RewardAccumulator` only exists once
    // a prior ClaimReward has already run the protocol-level reward setup
    // on this account.
    let Some(kl_buf) = ACCOUNT_KEYLET.take() else {
        RewardError::AssertionFailed.rollback(b"reward: account keylet buffer already taken");
    };
    let kl_len = raw::account_keylet(kl_buf, &sender);
    if kl_len < 0 {
        RewardError::AssertionFailed.rollback(b"reward: could not build account keylet");
    }
    let Some(kl) = kl_buf.get(..kl_len as usize) else {
        RewardError::AssertionFailed.rollback(b"reward: could not build account keylet");
    };
    if raw::slot_set(kl, 1) < 0 {
        RewardError::AssertionFailed.rollback(b"reward: could not slot sender account");
    }
    if raw::slot_subfield(1, sfRewardAccumulator, 2) != 2 {
        accept!(b"Reward: Passing reward setup txn", 0);
    }
    let has_first = raw::slot_subfield(1, sfRewardLgrFirst, 3) == 3;
    let has_last = raw::slot_subfield(1, sfRewardLgrLast, 4) == 4;
    let has_balance = raw::slot_subfield(1, sfBalance, 5) == 5;
    let has_time = raw::slot_subfield(1, sfRewardTime, 6) == 6;
    if !(has_first & has_last & has_balance & has_time) {
        RewardError::AssertionFailed.rollback(b"reward: missing reward accounting fields");
    }

    let last_claim_time = raw::slot_i64(6) as u64;
    let time_elapsed = ledger_last_time().wrapping_sub(last_claim_time);
    let required_delay = required_delay as u64;
    if time_elapsed < required_delay {
        let remaining = required_delay.wrapping_sub(time_elapsed);
        rollback!(&message::wait_message(remaining), 0);
    }

    let accumulator = raw::slot_i64(2);
    let first = raw::slot_i64(3);
    let last = raw::slot_i64(4);
    let raw_balance = raw::slot_i64(5) as u64;

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
    // reinterpretation C's implicit conversion performs).
    let xfl_accum = raw::float_set(0, accumulator);
    if xfl_accum <= 0 {
        RewardError::AssertionFailed.rollback(b"Reward: Assertion failure.");
    }
    let xfl_elapsed = raw::float_set(0, elapsed);
    if xfl_elapsed <= 0 {
        RewardError::AssertionFailed.rollback(b"Reward: Assertion failure.");
    }
    let per_ledger = raw::float_divide(xfl_accum, xfl_elapsed);
    let xfl_reward = raw::float_multiply(rr, per_ledger);
    let base_reward_drops = raw::float_int(xfl_reward, 6, 1) as u64;
    // `L1_SEATS` (20) is a nonzero compile-time constant; `wrapping_div`'s
    // only panicking case is a zero divisor, which this can never be —
    // clippy just can't see across the `mint_txn::L1_SEATS` import to
    // prove it.
    #[allow(clippy::arithmetic_side_effects)]
    let l1_drops = base_reward_drops.wrapping_div(L1_SEATS as u64);

    if raw::otxn_slot(10) != 10 {
        RewardError::AssertionFailed.rollback(b"reward: could not slot otxn");
    }
    if raw::slot_subfield(10, sfFee, 11) != 11 {
        RewardError::AssertionFailed.rollback(b"reward: could not slot otxn Fee");
    }
    // reward.c: `int64_t xfl_fee = slot_float(11);` — also unchecked.
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

    let mut emit_hash = Hash::zeroed();
    if raw::emit(emit_hash.as_mut(), bytes) < 0 {
        RewardError::EmitFailed.rollback(b"Reward: Emit loopback failed.");
    }
    accept!(b"Reward: Emitted reward txn successfully.", 0)
}

/// Appends one `GenesisMint` entry per L1 seat currently occupied by an
/// active validator (reward.c's `can_reward`/seat-iteration loops). Any
/// failure here is non-fatal to the overall claim (matching reward.c,
/// which treats the whole `UNLReport`-driven L1 distribution as
/// best-effort and always proceeds to emit at least the rewardee entry).
fn push_l1_seat_entries(txn: &mut MintTxn, l1_drops: u64) {
    if raw::slot_set(&UNLREPORT_KEYLET, 1) != 1 {
        return;
    }
    if raw::slot_subfield(1, sfActiveValidators, 1) != 1 {
        return;
    }
    let Some(av_buf) = AV_BUF.take() else {
        return;
    };
    if raw::slot(av_buf, 1) <= 0 {
        return;
    }
    let count = raw::slot_count(1);
    if count < 0 {
        return;
    }
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
        if raw::state(&mut seat, key) != 1 {
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
        if raw::state(destination.as_mut(), core::slice::from_ref(&this_seat)) != ACC_ID_LEN as i64
        {
            continue;
        }
        // At most `L1_SEATS` seat entries are ever pushed here (see
        // `mint_txn::MAX_ENTRIES`), so `push_entry` never actually hits its
        // internal overflow check in practice.
        txn.push_entry(l1_drops, &destination);
    }
}
