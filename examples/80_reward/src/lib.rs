//! Implements the Xahau `RewardHook`.
//!
//! On `ClaimReward`, it calculates the claimant's reward and emits a
//! `GenesisMint` for the claimant and active L1 governance seats.

#![no_std]

mod message;
mod mint_txn;
mod raw;

use hooks_lib::prelude::*;
use hooks_lib::static_cell::HookStatic;
use hooks_lib::*;
use mint_txn::{L1_SEATS, MintTxn};

metadata! {
    name: "reward",
    HookOn: [Invoke, ClaimReward],
    HookCanEmit: [GenesisMint],
}

/// Default reward rate, mirroring reward.c's default (~1/300 per claim).
const DEFAULT_REWARD_RATE: XFL = XFL!(0.003333333333333333);

/// Default reward delay: reward.c's own default of 2,600,000 seconds.
const DEFAULT_REWARD_DELAY: XFL = XFL!(2600000);

// Governance-controlled reward settings.
hook_state!(RewardRate, RewardRateKey = b"RR" => XFL);
hook_state!(RewardDelay, RewardDelayKey = b"RD" => XFL);

/// Maximum active validators in `UNLReport`.
const MAX_UNL: usize = 128;

/// Validator account offset within an `ActiveValidators` entry.
const AV_FIRST_KEY_OFFSET: usize = 39;

/// Size of an `ActiveValidators` entry.
const AV_ENTRY_STRIDE: usize = 60;

/// Maximum serialized `ActiveValidators` size.
const AV_ARRAY_LEN: usize = AV_ENTRY_STRIDE * MAX_UNL + 4;

/// `UNLReport` ledger keylet.
const UNLREPORT_KEYLET: [u8; 34] = [
    0x00, 0x52, 0x61, 0xE3, 0x2E, 0x7A, 0x24, 0xA2, 0x38, 0xF1, 0xC6, 0x19, 0xD5, 0xF9, 0xDD, 0xCC,
    0x41, 0xA9, 0x4B, 0x33, 0xB6, 0x6C, 0x01, 0x63, 0xF7, 0xEF, 0xCC, 0x8A, 0x19, 0xC9, 0xFD, 0x6F,
    0x28, 0xDC,
];

hook_errors! {
    /// `reward` rollback reasons.
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

/// The five account-root fields this hook reads, and which of the three
/// "cannot proceed" outcomes applies instead.
enum RewardFieldRead {
    /// Every field was present.
    Read(RewardFields),
    /// The sender's account root could not be loaded at all.
    NoAccountSlot,
    /// No `sfRewardAccumulator`: this is a reward *setup* transaction, which
    /// the hook passes rather than rejects.
    SetupTxn,
    /// The accumulator is there but the rest of the accounting is not.
    MissingFields,
}

/// The account-root values the reward calculation needs.
struct RewardFields {
    accumulator: i64,
    first: i64,
    last: i64,
    raw_balance: u64,
    last_claim_time: u64,
}

/// Reads the sender's reward accounting off their account root.
///
/// `#[inline(never)]`, and that is load-bearing rather than stylistic: this
/// hook sits near the Hook API's 32-level block-nesting ceiling (22 of 32
/// before this change), and five `Result`-returning typed reads inlined into
/// `my_hook` measured **68**. In its own frame the same code costs nesting
/// the entry point never sees. `examples/81_govern` and
/// `examples/15_slot-objects` use the identical escape hatch; see
/// `docs/DESIGN.md` §5.8.
#[inline(never)]
fn read_reward_fields(keylet: &Keylet) -> RewardFieldRead {
    let Ok(account) = SlotObject::from_keylet(keylet) else {
        return RewardFieldRead::NoAccountSlot;
    };
    let Ok(accumulator_slot) = account.get(sfRewardAccumulator) else {
        return RewardFieldRead::SetupTxn;
    };
    // Four sequential `let else`s rather than one tuple pattern: a 4-way
    // tuple `let (Ok(..), Ok(..), Ok(..), Ok(..)) = .. else` lowers to
    // nested matches, and this hook has no nesting to spare.
    let Ok(first_slot) = account.get(sfRewardLgrFirst) else {
        return RewardFieldRead::MissingFields;
    };
    let Ok(last_slot) = account.get(sfRewardLgrLast) else {
        return RewardFieldRead::MissingFields;
    };
    let Ok(balance_slot) = account.get(sfBalance) else {
        return RewardFieldRead::MissingFields;
    };
    let Ok(time_slot) = account.get(sfRewardTime) else {
        return RewardFieldRead::MissingFields;
    };

    RewardFieldRead::Read(RewardFields {
        accumulator: accumulator_slot.value().unwrap_or(0) as i64,
        first: i64::from(first_slot.value().unwrap_or(0)),
        last: i64::from(last_slot.value().unwrap_or(0)),
        // Read the native amount's raw wire encoding.
        raw_balance: balance_slot.assume_type::<u64>().value().unwrap_or(0),
        last_claim_time: u64::from(time_slot.value().unwrap_or(0)),
    })
}

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

    let Ok(sender) = otxn_field_typed(sfAccount) else {
        RewardError::AssertionFailed.rollback(b"reward: could not read otxn Account")
    };
    let Ok(hook_acc) = hook_account_buf() else {
        RewardError::AssertionFailed.rollback(b"reward: could not read hook_account")
    };
    // The hook's own emitted ClaimReward-adjacent traffic (there is none
    // today, but reward.c guards this unconditionally) passes through.
    if buf_eq_20(&sender, &hook_acc) {
        accept!(b"Reward: Passing outgoing txn", 0);
    }

    let rr = RewardRate
        .get_state()
        .unwrap_or(None)
        .unwrap_or(DEFAULT_REWARD_RATE);
    let rd = RewardDelay
        .get_state()
        .unwrap_or(None)
        .unwrap_or(DEFAULT_REWARD_DELAY);
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
    // on this account. The typed helper returns a full 34-byte `Keylet`
    // by construction, so no length check is needed.
    let Ok(kl) = keylet_account(&sender) else {
        RewardError::AssertionFailed.rollback(b"reward: could not build account keylet")
    };
    let fields = match read_reward_fields(&kl) {
        RewardFieldRead::Read(f) => f,
        RewardFieldRead::NoAccountSlot => {
            RewardError::AssertionFailed.rollback(b"reward: could not slot sender account")
        }
        RewardFieldRead::SetupTxn => accept!(b"Reward: Passing reward setup txn", 0),
        RewardFieldRead::MissingFields => {
            RewardError::AssertionFailed.rollback(b"reward: missing reward accounting fields")
        }
    };
    let last_claim_time = fields.last_claim_time;

    let time_elapsed = ledger_last_time().wrapping_sub(last_claim_time);
    let required_delay = required_delay as u64;
    if time_elapsed < required_delay {
        let remaining = required_delay.wrapping_sub(time_elapsed);
        rollback!(&message::wait_message(remaining), 0);
    }

    let accumulator = fields.accumulator;
    let first = fields.first;
    let last = fields.last;
    let raw_balance = fields.raw_balance;

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

    let Ok(otxn) = SlotObject::from_otxn() else {
        RewardError::AssertionFailed.rollback(b"reward: could not slot otxn");
    };
    let Ok(fee_slot) = otxn.get(sfFee) else {
        RewardError::AssertionFailed.rollback(b"reward: could not slot otxn Fee");
    };
    // reward.c: `int64_t xfl_fee = slot_float(11);` — unchecked, folding a
    // failed call into the same `> 0` test the value needs anyway (see
    // [`raw`]'s doc comment). `as_xfl()` is the checked spelling, so the
    // fold happens here instead: a failure becomes `0`, which fails the same
    // test a negative error code did.
    let xfl_fee = fee_slot.as_xfl().map(XFL::raw_bits).unwrap_or(0);
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
    let Ok(unl_report) = SlotObject::from_keylet(&Keylet(UNLREPORT_KEYLET)) else {
        return;
    };
    // `sfActiveValidators` is an `STArray`, so the handle knows it has a
    // `count()`.
    let Ok(validators) = unl_report.get(sfActiveValidators) else {
        return;
    };
    let Some(av_buf) = AV_BUF.take() else {
        return;
    };
    // `count()` borrows, so it must come before the consuming `raw()` read —
    // which is exactly the ordering the borrowing pre-checks exist for.
    let Ok(count) = validators.count() else {
        return;
    };
    let Ok(read_len) = validators.raw(av_buf) else {
        return;
    };
    if read_len == 0 {
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
