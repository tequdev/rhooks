//! Implements a 20-seat L1/L2 governance Hook.
//!
//! Members vote on seats, hooks, and reward settings. Reaching the voting
//! threshold applies the change or forwards an L2 vote to L1.

#![no_std]

mod keys;
mod txn;

use hooks_lib::prelude::*;
use hooks_lib::slot_path;
use hooks_lib::static_cell::HookStatic;
use hooks_lib::*;

// Per-seat initial member parameter.
hook_parameter!(MemberParam, MemberParamName [u8; 3] => AccountId);

// Setup parameters.
hook_parameter!(InitialMemberCount, InitialMemberCountParamName = b"IMC" => [u8; 1]);
hook_parameter!(InitialRewardRate, InitialRewardRateParamName = b"IRR" => XFL);
hook_parameter!(InitialRewardDelay, InitialRewardDelayParamName = b"IRD" => XFL);

// Per-vote topic and layer parameters.
otxn_parameter!(TopicParam, TopicParamName = b"T" => [u8; 2]);
otxn_parameter!(LayerParam, LayerParamName = b"L" => [u8; 1]);

/// Network genesis account.
const GENESIS_ACCOUNT: AccountId = account_id!("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh");

/// `SEAT_COUNT` in govern.c: the round table has 20 seats.
const SEAT_COUNT: u8 = 20;
/// `HOOK_MAX` in govern.c: hook topics `0..=10` are accepted (govern.c's
/// own `n > HOOK_MAX` check lets `n == 10` through too, even though only
/// slots `0..=9` exist on a 10-slot `Hooks` array — preserved exactly;
/// see the README's differences table).
const HOOK_MAX: u8 = 10;

hook_errors! {
    /// `govern`'s rollback reasons. See `examples/80_reward`'s
    /// `RewardError` doc comment for why these codes (not govern.c's
    /// `__LINE__`) are the stable ones, and why only `accept`/`rollback`
    /// outcome + message are the behavior-equivalence target.
    pub enum GovernError {
        /// A hook parameter or otxn parameter required by the current
        /// step was missing or malformed.
        BadParameter = -201,
        /// The invoking account is not a member of this table.
        NotAMember = -202,
        /// An invariant govern.c enforces with `ASSERT` failed —
        /// defensive; unreachable for well-formed state.
        AssertionFailed = -203,
        /// Building or submitting an emitted transaction failed.
        EmitFailed = -204,
    }
}

// Scratch buffers for transaction and state data.
static TOPIC_DATA: HookStatic<[u8; 32]> = HookStatic::new([0u8; 32]);
static PREVIOUS_TOPIC_DATA: HookStatic<[u8; 32]> = HookStatic::new([0u8; 32]);
static PREVIOUS_MEMBER: HookStatic<[u8; 32]> = HookStatic::new([0u8; 32]);
static VOTE_VALUE: HookStatic<[u8; 32]> = HookStatic::new([0u8; 32]);

/// Takes a scratch buffer.
#[inline(always)]
fn take_scratch<T>(cell: &'static HookStatic<T>) -> &'static mut T {
    let Some(buf) = cell.take() else {
        GovernError::AssertionFailed.nope(b"govern: scratch buffer already taken");
    };
    buf
}

impl GovernError {
    #[inline(always)]
    fn nope(self, msg: &[u8]) -> ! {
        rollback!(msg, self)
    }
}

#[inline(always)]
fn done(msg: &[u8]) -> ! {
    accept!(msg, 0)
}

#[hook]
fn my_hook() -> i64 {
    if etxn_reserve(1).is_err() {
        GovernError::EmitFailed.nope(b"govern: etxn_reserve failed");
    }

    if otxn_type() != TxType::Invoke {
        done(b"Governance: Passing non-Invoke txn. HookOn should be changed to avoid this.");
    }

    let Ok(sender) = otxn_field_exact::<AccountId>(sfAccount) else {
        GovernError::AssertionFailed.nope(b"govern: could not read otxn Account")
    };
    let Ok(hook_accid) = hook_account_buf() else {
        GovernError::AssertionFailed.nope(b"govern: could not read hook_account")
    };

    if buf_eq_20(&sender, &hook_accid) {
        if let Ok(dest) = otxn_field_exact::<AccountId>(sfDestination) {
            if !buf_eq_20(&hook_accid, &dest) {
                done(b"Goverance: Passing outgoing txn.");
            }
        }
    }

    let is_l1_table = buf_eq_20(&hook_accid, &GENESIS_ACCOUNT);

    // `state_u64`, not `state_i64`: govern.c's own `state_i64(key, len)` is
    // actually the Hook API's "as-int64" `state(0, 0, key, len)` idiom (the
    // host packs whatever bytes *are* stored, regardless of their actual
    // length, into the return value) — the right hooks-lib match for that
    // is [`state_u64`] (see its doc comment), not [`state_i64`] (which
    // requires an exact 8-byte stored entry via [`state_exact`] and would
    // therefore *always* fail on `"MC"`'s actual 1-byte stored value).
    //
    // Deliberately `Err(_)`, not `Err(HookError::DoesntExist)`: pattern
    // matching a *specific* `HookError` variant forces the compiler to
    // fully resolve `hooks_lib::error::res`'s ~40-arm `HookError::from(i64)`
    // decode at this call site (measured: pushes local nesting depth to
    // 56, over the 32-level limit — see `crate`'s module doc comment).
    // Testing only `is_err()`-equivalent (never reading which specific
    // error occurred) lets the optimizer discard that decode entirely,
    // keeping just the "is the raw code negative" branch — this crate's
    // one and only edit made purely to stay under the nesting limit, and
    // it changes nothing observable: any `state_u64` failure on the fixed
    // 2-byte `"MC"` key is unreachable other than "value not yet written"
    // (`DoesntExist`) for a well-formed table, matching govern.c's own
    // `== DOESNT_EXIST` check in every reachable case.
    let member_count = match state_u64(&keys::MEMBER_COUNT) {
        Ok(v) => v as i64,
        Err(_) => setup(is_l1_table),
    };

    // Same "as-int64" mode as `member_count` above (a member-reverse
    // entry's value is a 1-byte seat number).
    let member_id = state_u64(&keys::member_reverse_key(&sender))
        .map(|v| v as i64)
        .unwrap_or(-1);
    if member_id < 0 {
        GovernError::NotAMember
            .nope(b"Governance: You are not currently a governance member at this table.");
    }

    let topic_result: Result<[u8; 2]> = TopicParam.get_value();
    let topic_ok = topic_result.is_ok();
    let topic = topic_result.unwrap_or([0, 0]);
    let t = topic[0];
    let n = topic[1];
    if !topic_ok || (t != b'S' && t != b'H' && t != b'R') {
        GovernError::BadParameter
            .nope(b"Governance: Valid TOPIC must be specified as otxn parameter.");
    }
    if t == b'S' && n > SEAT_COUNT.wrapping_sub(1) {
        GovernError::BadParameter.nope(b"Governance: Valid seat topics are 0 through 19.");
    }
    if t == b'H' && n > HOOK_MAX {
        GovernError::BadParameter.nope(b"Governance: Valid hook topics are 0 through 9.");
    }
    if t == b'R' && n != b'R' && n != b'D' {
        GovernError::BadParameter
            .nope(b"Governance: Valid reward topics are R (rate) and D (delay).");
    }

    let mut l = 1u8;
    if !is_l1_table {
        l = match LayerParam.get_value() {
            Ok([v]) => v,
            Err(_) => GovernError::BadParameter
                .nope(b"Governance: Missing L parameter. Which layer are you voting for?"),
        };
        if l != 1 && l != 2 {
            GovernError::BadParameter.nope(b"Governance: Layer parameter must be '1' or '2'.");
        }
    }
    if l == 2 && t == b'R' {
        GovernError::BadParameter
            .nope(b"Governance: L2s cannot vote on RR/RD at L2, did you mean to set L=1?");
    }

    let topic_size: usize = if t == b'H' {
        32
    } else if t == b'S' {
        20
    } else {
        8
    };
    let padding = 32usize.wrapping_sub(topic_size);

    let topic_data = take_scratch(&TOPIC_DATA);
    let vresult = {
        let Some(dst) = topic_data.get_mut(padding..) else {
            GovernError::AssertionFailed.nope(b"govern: bad topic padding");
        };
        otxn_param(dst, b"V")
    };
    if vresult != Ok(topic_size) {
        GovernError::BadParameter
            .nope(b"Governance: Missing or incorrect size of VOTE data for TOPIC type.");
    }
    let topic_data_zero = buf_eq_32(topic_data, &[0u8; 32]);

    let vk = keys::vote_key(t, n, l, &sender);
    let previous_topic_data = take_scratch(&PREVIOUS_TOPIC_DATA);
    let previous_topic_size = {
        let Some(dst) = previous_topic_data.get_mut(padding..) else {
            GovernError::AssertionFailed.nope(b"govern: bad topic padding");
        };
        state(dst, &vk).unwrap_or(0)
    };

    if previous_topic_size == topic_size && buf_eq_32(previous_topic_data, topic_data) {
        done(b"Governance: Your vote is already cast this way for this topic.");
    }

    {
        let Some(value) = topic_data.get(padding..) else {
            GovernError::AssertionFailed.nope(b"govern: bad topic padding");
        };
        if state_set(value, &vk) != Ok(topic_size) {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
    }

    if previous_topic_size > 0 {
        let Some(prev_value) = previous_topic_data.get(padding..) else {
            GovernError::AssertionFailed.nope(b"govern: bad topic padding");
        };
        let vck = keys::vote_count_key(t, n, l, prev_value);
        let mut votes = [0u8; 1];
        if state(&mut votes, &vck) != Ok(1) {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
        if votes[0] == 0 {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
        if votes[0] <= 1 {
            if state_set(&[], &vck).is_err() {
                GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
            }
        } else {
            let dec = votes[0].wrapping_sub(1);
            if state_set(&[dec], &vck) != Ok(1) {
                GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
            }
        }
    }

    let votes = {
        let Some(new_value) = topic_data.get(padding..) else {
            GovernError::AssertionFailed.nope(b"govern: bad topic padding");
        };
        let vck = keys::vote_count_key(t, n, l, new_value);
        let mut vc = [0u8; 1];
        let _ = state(&mut vc, &vck); // ignored on failure, matching govern.c
        let new_votes = vc[0].wrapping_add(1);
        if state_set(&[new_votes], &vck).is_err() {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
        new_votes
    };

    // govern.c computes these via `int64_t q80 = member_count * 0.8;`
    // (hardware `double` multiplication, truncated toward zero on
    // assignment) — reproduced here as exact rational integer arithmetic
    // (`* 4 / 5`, `* 51 / 100`) instead: the Hook API's guard checker
    // rejects wasm floating-point opcodes outright (`f64.mul` et al. are
    // not in the allowed instruction set for a Guard-type hook), and for
    // every `member_count` this hook ever sees (`0..=SEAT_COUNT`, i.e.
    // `0..=20`) `member_count * 4 / 5` and `member_count * 0.8` truncated
    // agree exactly — `0.8`'s IEEE-754 `double` representation is close
    // enough to exact that no `member_count` in range lands within
    // rounding distance of an integer boundary.
    let mut q80 = member_count.wrapping_mul(4).wrapping_div(5);
    let mut q51 = member_count.wrapping_mul(51).wrapping_div(100);
    if q80 < 2 {
        q80 = 2;
    }
    if q51 < 2 {
        q51 = 2;
    }

    if is_l1_table || l == 2 {
        let threshold = if t == b'S' { q80 } else { member_count };
        if i64::from(votes) < threshold {
            done(b"Governance: Vote record. Not yet enough votes to action.");
        }
    } else if i64::from(votes) < q51 {
        done(b"Governance: Not yet enough votes to action L1 vote...");
    }

    if l == 1 && !is_l1_table {
        let Some(value) = topic_data.get(padding..) else {
            GovernError::AssertionFailed.nope(b"govern: bad topic padding");
        };
        if txn::emit_l1_vote_forward(&hook_accid, &GENESIS_ACCOUNT, t, n, topic_size as u8, value) {
            done(b"Governance: Successfully emitted L1 vote.");
        }
        GovernError::EmitFailed.nope(b"Governance: L1 vote emission failed.");
    }

    if t == b'R' {
        action_reward(t, n, padding, topic_data);
    } else if t == b'H' {
        action_hook(&hook_accid, n, topic_data_zero, topic_data);
    } else {
        action_seat(n, topic_data_zero, topic_data);
    }
}

/// Reads `IRR`/`IRD` (L1-table-only setup) and writes `"RR"`/`"RD"` state.
/// Kept in its own `#[inline(never)]` function: reading both via
/// `hook_param_typed` inline inside `setup` pushes `setup`'s own compiled
/// nesting to 56 (over the 32-level limit) — `hooks-build`'s unnest pass
/// is sensitive to a function's overall compiled shape, not just each
/// call site's isolated cost. This function boundary keeps nesting at 22.
#[inline(never)]
fn setup_initial_reward_rate_and_delay() {
    let Ok(irr) = InitialRewardRate.get_value() else {
        GovernError::BadParameter.nope(b"Governance: Initial Reward Rate Parameter missing (IRR).")
    };
    let Ok(ird) = InitialRewardDelay.get_value() else {
        GovernError::BadParameter.nope(b"Governance: Initial Reward Delay Parameter miss (IRD).")
    };
    if ird.raw_bits() == 0 {
        GovernError::BadParameter.nope(b"Governance: Initial Reward Delay must be > 0.");
    }
    if state_set(&irr.raw_bits().to_le_bytes(), &keys::REWARD_RATE).is_err() {
        GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
    }
    if state_set(&ird.raw_bits().to_le_bytes(), &keys::REWARD_DELAY).is_err() {
        GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
    }
}

/// First-ever `Invoke` on this table: reads `IMC` (+`IRR`/`IRD` on L1)
/// and `IS0..IS{imc-1}` hook parameters and populates the initial seat
/// table. Diverges (`accept!`/`rollback!`) — govern.c's setup path never
/// falls through to normal voting.
///
/// `IMC`/`IRR`/`IRD` are read via `hook_parameter!`'s typed accessors —
/// an intentional behavior difference from govern.c, not a byte-for-
/// byte-equivalent port; see the declarations above and the README's
/// "Parameter read semantics" section for the full argument, and
/// `e2e/test/govern.test.ts`'s "rejects a too-short IRR value..." test
/// for the regression guard.
#[inline(never)]
fn setup(is_l1_table: bool) -> ! {
    let Ok(imc) = InitialMemberCount.get_value() else {
        GovernError::BadParameter.nope(b"Governance: Initial Member Count Parameter missing (IMC).")
    };
    if state_set(&imc, &keys::MEMBER_COUNT).is_err() {
        GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
    }
    let member_count = imc[0];
    if member_count == 0 {
        GovernError::BadParameter.nope(b"Governance: Initial Member Count must be > 0.");
    }
    if member_count > SEAT_COUNT {
        GovernError::BadParameter
            .nope(b"Governance: Initial Member Count must be <= Seat Count (20).");
    }

    if is_l1_table {
        setup_initial_reward_rate_and_delay();
    }

    let mut i = 0u8;
    while i < member_count {
        guard!(u32::from(SEAT_COUNT));
        let this_seat = i;
        i = i.wrapping_add(1);
        let member_pkey = MemberParam([b'I', b'S', this_seat]);
        let Ok(member_acc) = member_pkey.get_value() else {
            GovernError::BadParameter
                .nope(b"Governance: One or more initial member account ID's is missing")
        };
        if state_set(member_acc.as_ref(), &keys::seat_forward_key(this_seat)) != Ok(20) {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
        if state_set(&[this_seat], &keys::member_reverse_key(&member_acc)) != Ok(1) {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
    }

    done(b"Governance: Setup completed successfully.");
}

/// Actions a reward-rate/delay topic (`t == 'R'`): writes the voted value
/// directly under the `"RR"`/`"RD"` key. Diverges.
#[inline(never)]
fn action_reward(_t: u8, n: u8, padding: usize, topic_data: &[u8; 32]) -> ! {
    let Some(value) = topic_data.get(padding..) else {
        GovernError::AssertionFailed.nope(b"govern: bad topic padding");
    };
    let key = if n == b'R' {
        keys::REWARD_RATE
    } else {
        keys::REWARD_DELAY
    };
    if state_set(value, &key).is_err() {
        GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
    }
    if n == b'R' {
        done(b"Governance: Reward rate change actioned!");
    }
    done(b"Governance: Reward delay change actioned!");
}

/// Actions a hook topic (`t == 'H'`): installs or deletes hook slot `n`
/// via an emitted `HookSet`. Diverges.
#[inline(never)]
fn action_hook(hook_accid: &AccountId, n: u8, topic_data_zero: bool, topic_data: &[u8; 32]) -> ! {
    let Ok(keylet) = keylet_hook(hook_accid) else {
        GovernError::AssertionFailed.nope(b"govern: could not build hook keylet");
    };
    // The already-installed hook's hash, if there is one: the hook account's
    // `Hooks` array, element `n`, its `HookHash`. `slot_path!` clears each
    // intermediate as soon as its child exists, so this costs one live slot
    // rather than three — and it flattens to a single `if let` here, which
    // matters in a hook this close to the nesting ceiling.
    //
    // Missing hook data skips the comparison.
    if let Ok(hook_acc) = SlotObject::from_keylet(&keylet) {
        if let Ok(hash_slot) = slot_path!(hook_acc[sfHooks][u32::from(n)][sfHookHash]) {
            let Ok(existing_hook) = hash_slot.value() else {
                GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
            };
            if buf_eq_32(&existing_hook, topic_data) {
                done(b"Goverance: Target hook is already the same as actioned hook.");
            }
        }
        let _ = hook_acc.clear();
    }

    if !topic_data_zero {
        let Ok(hdef_keylet) = keylet_hook_definition(&Hash::from(*topic_data)) else {
            GovernError::AssertionFailed.nope(b"govern: could not build hook definition keylet");
        };
        // Existence check only — the handle is dropped without reading, and
        // the slot lives until the hook ends (the C cost model; see
        // `hooks_lib::slot_obj`).
        if SlotObject::from_keylet(&hdef_keylet).is_err() {
            GovernError::BadParameter
                .nope(b"Goverance: Hook Hash doesn't exist on ledger while actioning hook.");
        }
    }

    let hash_opt = if topic_data_zero {
        None
    } else {
        Some(topic_data)
    };
    if txn::emit_hookset(hook_accid, n, hash_opt) {
        done(b"Governance: Hook actioned.");
    }
    GovernError::EmitFailed.nope(b"Governance: Emit failed during hook actioning.");
}

/// Actions a seat topic (`t == 'S'`): adds, moves, or removes a member —
/// govern.c's full 8-case `E|Z|M` logic table, including the vote
/// garbage-collection double loop over the outgoing member's votes.
/// Diverges.
#[inline(never)]
fn action_seat(n: u8, topic_data_zero: bool, topic_data: &[u8; 32]) -> ! {
    let Some(new_account) = topic_data.get(12..32) else {
        GovernError::AssertionFailed.nope(b"govern: bad topic data");
    };

    let previous_member = take_scratch(&PREVIOUS_MEMBER);
    let previous_present = {
        let Some(dst) = previous_member.get_mut(12..32) else {
            GovernError::AssertionFailed.nope(b"govern: bad buffer");
        };
        state(dst, &keys::seat_forward_key(n)) == Ok(20)
    };

    if previous_present {
        let Some(prev_account) = previous_member.get(12..32) else {
            GovernError::AssertionFailed.nope(b"govern: bad buffer");
        };
        if bytes_eq(prev_account, new_account) {
            done(b"Governance: Actioning seat change, but seat already contains the new member.");
        }
    }

    // "as-int64" mode again — see `my_hook`'s `member_count` doc comment.
    let existing_member = state_u64(new_account).map(|v| v as i64).unwrap_or(-1);
    let existing_member_moving = existing_member >= 0;

    let op: u8 = (u8::from(!previous_present) << 2)
        | (u8::from(topic_data_zero) << 1)
        | u8::from(existing_member_moving);
    if op == 0b011 || op == 0b111 {
        GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
    }

    let mut member_count = state_u64(&keys::MEMBER_COUNT)
        .map(|v| v as i64)
        .unwrap_or(0);
    if op == 0b001 || op == 0b010 {
        member_count = member_count.wrapping_sub(1);
    } else if op == 0b100 {
        member_count = member_count.wrapping_add(1);
    }
    if member_count <= 1 {
        GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
    }
    let mc = member_count as u8;
    if state_set(&[mc], &keys::MEMBER_COUNT) != Ok(1) {
        GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
    }

    if existing_member_moving {
        let m = existing_member as u8;
        if state_set(&[], &keys::seat_forward_key(m)).is_err() {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
        if state_set(&[], new_account).is_err() {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
    }

    if previous_present {
        previous_member[0] = b'V';
        let vote_value = take_scratch(&VOTE_VALUE);
        let mut tbl = 1u8;
        while tbl <= 2 {
            guard!(2);
            let this_tbl = tbl;
            tbl = tbl.wrapping_add(1);
            garbage_collect_votes(previous_member, vote_value, this_tbl);
        }

        if state_set(&[], &keys::seat_forward_key(n)).is_err() {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
        let Some(prev_account) = previous_member.get(12..32) else {
            GovernError::AssertionFailed.nope(b"govern: bad buffer");
        };
        if state_set(&[], prev_account).is_err() {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
    }

    if !topic_data_zero {
        if state_set(new_account, &keys::seat_forward_key(n)) != Ok(20) {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
        if state_set(&[n], new_account) != Ok(1) {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
    }

    done(b"Governance: Action member change.");
}

/// The inner loop of `action_seat`'s vote garbage collection: for table
/// `tbl` (1 or 2), scans all 32 topic slots (2 reward + 10 hook + 20
/// seat) the outgoing member (`previous_member[12..32)`) may have voted
/// on, deleting the vote and decrementing/deleting the vote-count entry.
/// `previous_member[0]` must already be `'V'`; this function overwrites
/// `[1..4)` on every iteration (`previous_member` doubles as the vote key
/// throughout, matching govern.c's in-place reuse of the same buffer).
#[inline(never)]
fn garbage_collect_votes(previous_member: &mut [u8; 32], vote_value: &mut [u8; 32], tbl: u8) {
    let mut i = 0u8;
    while i < 32 {
        // `66`, not `32`: the Hook API's guard-iteration counter is
        // scoped to the *static* `_g` call site (keyed by source line),
        // not to each dynamic loop entry — this function runs twice per
        // `action_seat` call (`tbl` 1 and 2), so the two calls' iteration
        // counts *accumulate* against this one guard site within a
        // single hook execution. `32` alone was confirmed live (via this
        // suite's e2e run) to trip a `GUARD_VIOLATION` on the second
        // `tbl` pass at cumulative iteration 34; `66` is govern.c's own
        // `GUARD(66)` bound for this exact loop, which turns out to
        // encode precisely this accumulation (`2 * 32` plus slack), not
        // arbitrary generosity as first assumed — see the README's
        // differences table.
        guard!(66);
        let this_i = i;
        i = i.wrapping_add(1);

        let topic_type = if this_i < 2 {
            b'R'
        } else if this_i < 12 {
            b'H'
        } else {
            b'S'
        };
        let topic_id = if this_i == 0 {
            b'R'
        } else if this_i == 1 {
            b'D'
        } else if this_i < 12 {
            this_i.wrapping_sub(2)
        } else {
            this_i.wrapping_sub(12)
        };
        previous_member[1] = topic_type;
        previous_member[2] = topic_id;
        previous_member[3] = tbl;

        let topic_size: usize = if topic_type == b'H' {
            32
        } else if topic_type == b'S' {
            20
        } else {
            8
        };
        let padding = 32usize.wrapping_sub(topic_size);

        let read = {
            let Some(dst) = vote_value.get_mut(padding..) else {
                continue;
            };
            state(dst, previous_member.as_ref())
        };
        if read != Ok(topic_size) {
            continue;
        }

        let Some(value) = vote_value.get(padding..) else {
            continue;
        };
        let vck = keys::vote_count_key(topic_type, topic_id, tbl, value);
        let mut vote_count = [0u8; 1];
        if state(&mut vote_count, &vck) == Ok(1) {
            if vote_count[0] <= 1 {
                let _ = state_set(&[], &vck);
            } else {
                let dec = vote_count[0].wrapping_sub(1);
                let _ = state_set(&[dec], &vck);
            }
        }
        let _ = state_set(&[], previous_member.as_ref());
    }
}

/// Element-wise byte-slice equality, no `copy_from_slice`/length-mismatch
/// panic path (see `examples/80_reward/src/mint_txn.rs::MintTxn::push`'s
/// doc comment for why that matters for this crate's nesting budget) —
/// used for the handful of runtime-length (not fixed-`N`) comparisons
/// `buf_eq_20`/`buf_eq_32` can't cover.
#[inline(always)]
fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    let mut eq = true;
    while i < a.len() {
        guard!(32);
        if a.get(i) != b.get(i) {
            eq = false;
        }
        i = i.wrapping_add(1);
    }
    eq
}
