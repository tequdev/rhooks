//! `govern` — a behavior-equivalent Rust port of xahaud's genesis
//! `GovernanceHook` (`hook/genesis/govern.c`).
//!
//! A 20-seat round-table governance hook. Installed on the genesis
//! (L1) account it is the *L1 table*; installed on any other
//! (blackholed) account it is an *L2 table*. Members vote on topics
//! (seats, hooks, reward rate/delay); once a topic's votes cross a
//! threshold, the vote is "actioned" — a seat/hook/reward-parameter
//! change is applied, or (for an L2 table voting on an L1 topic) the
//! vote is forwarded to L1 as an `Invoke`. See the README for the state
//! layout, hook-parameter table, and full behavior-equivalence table
//! (govern.c branch -> Rust here -> observable outcome).
//!
//! See `examples/80_reward`'s crate doc comment for why every fallible
//! Hook API call here goes through [`raw`] instead of
//! `hooks_lib::api`'s `Result<_, HookError>` wrappers — the identical
//! Guard-type nesting-depth constraint applies here too, and even more
//! so (govern.c is the larger of the two genesis hooks).
//!
//! Build: `hooks-build build --manifest-path examples/81_govern/Cargo.toml`

#![no_std]

mod keys;
mod raw;
mod txn;

use hooks_lib::prelude::*;
use hooks_lib::raw::DOESNT_EXIST;
use hooks_lib::{accept, guard, hook, hook_errors, rollback};

/// `genesis[20]` in govern.c: the network genesis account (see
/// `examples/80_reward/src/mint_txn.rs::GENESIS_ACCOUNT`'s doc comment
/// for how this was verified — `secp256k1
/// calcAccountID(generateSeed("masterpassphrase"))`).
const GENESIS_ACCOUNT: AccountId = AccountId([
    0xB5, 0xF7, 0x62, 0x79, 0x8A, 0x53, 0xD5, 0x43, 0xA0, 0x14, 0xCA, 0xF8, 0xB2, 0x97, 0xCF, 0xF8,
    0xF2, 0xF9, 0x37, 0xE8,
]);

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

// Scratch buffers for 32-byte-and-up values used at various points below,
// each a `HookStatic` rather than a stack local: `wasm32v1-none`'s
// codegen starts lowering a zero-initialized stack array of this size to
// an unguarded `memset`-style loop (see `examples/README.md`'s "Statics
// for templates and large buffers", and `examples/80_reward/src/lib.rs`'s
// `ACCOUNT_KEYLET` for the same fix there).
use hooks_lib::static_cell::HookStatic;

static TOPIC_DATA: HookStatic<[u8; 32]> = HookStatic::new([0u8; 32]);
static PREVIOUS_TOPIC_DATA: HookStatic<[u8; 32]> = HookStatic::new([0u8; 32]);
static HOOK_KEYLET: HookStatic<[u8; 34]> = HookStatic::new([0u8; 34]);
static HOOK_DEFINITION_KEYLET: HookStatic<[u8; 34]> = HookStatic::new([0u8; 34]);
static EXISTING_HOOK: HookStatic<[u8; 32]> = HookStatic::new([0u8; 32]);
static PREVIOUS_MEMBER: HookStatic<[u8; 32]> = HookStatic::new([0u8; 32]);
static VOTE_VALUE: HookStatic<[u8; 32]> = HookStatic::new([0u8; 32]);

/// Takes a scratch [`HookStatic`] buffer, rolling back if it was somehow
/// already taken (never happens in practice — each one is taken exactly
/// once per hook execution).
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
    if raw::etxn_reserve(1) < 0 {
        GovernError::EmitFailed.nope(b"govern: etxn_reserve failed");
    }

    if u32::from(otxn_type()) != ttINVOKE {
        done(b"Governance: Passing non-Invoke txn. HookOn should be changed to avoid this.");
    }

    let mut sender = AccountId::zeroed();
    if raw::otxn_field(sender.as_mut(), sfAccount) != 20 {
        GovernError::AssertionFailed.nope(b"govern: could not read otxn Account");
    }
    let mut hook_accid = AccountId::zeroed();
    if raw::hook_account(hook_accid.as_mut()) != 20 {
        GovernError::AssertionFailed.nope(b"govern: could not read hook_account");
    }

    if buf_eq_20(&sender, &hook_accid) {
        let mut dest = AccountId::zeroed();
        if raw::otxn_field(dest.as_mut(), sfDestination) == 20 && !buf_eq_20(&hook_accid, &dest) {
            done(b"Goverance: Passing outgoing txn.");
        }
    }

    let is_l1_table = buf_eq_20(&hook_accid, &GENESIS_ACCOUNT);

    let member_count_raw = raw::state_i64(&keys::MEMBER_COUNT);
    if member_count_raw == DOESNT_EXIST {
        setup(is_l1_table);
    }
    let member_count = member_count_raw;

    let member_id = raw::state_i64(&keys::member_reverse_key(&sender));
    if member_id < 0 {
        GovernError::NotAMember
            .nope(b"Governance: You are not currently a governance member at this table.");
    }

    let mut topic = [0u8; 2];
    let topic_result = raw::otxn_param(&mut topic, b"T");
    let t = topic[0];
    let n = topic[1];
    if topic_result != 2 || (t != b'S' && t != b'H' && t != b'R') {
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
        let mut lbuf = [0u8; 1];
        if raw::otxn_param(&mut lbuf, b"L") != 1 {
            GovernError::BadParameter
                .nope(b"Governance: Missing L parameter. Which layer are you voting for?");
        }
        l = lbuf[0];
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
        raw::otxn_param(dst, b"V")
    };
    if vresult != topic_size as i64 {
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
        raw::state(dst, &vk)
    };

    if previous_topic_size == topic_size as i64 && buf_eq_32(previous_topic_data, topic_data) {
        done(b"Governance: Your vote is already cast this way for this topic.");
    }

    {
        let Some(value) = topic_data.get(padding..) else {
            GovernError::AssertionFailed.nope(b"govern: bad topic padding");
        };
        if raw::state_set(value, &vk) != topic_size as i64 {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
    }

    if previous_topic_size > 0 {
        let Some(prev_value) = previous_topic_data.get(padding..) else {
            GovernError::AssertionFailed.nope(b"govern: bad topic padding");
        };
        let vck = keys::vote_count_key(t, n, l, prev_value);
        let mut votes = [0u8; 1];
        if raw::state(&mut votes, &vck) != 1 {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
        if votes[0] == 0 {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
        if votes[0] <= 1 {
            if raw::state_delete(&vck) < 0 {
                GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
            }
        } else {
            let dec = votes[0].wrapping_sub(1);
            if raw::state_set(&[dec], &vck) != 1 {
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
        let _ = raw::state(&mut vc, &vck); // ignored on failure, matching govern.c
        let new_votes = vc[0].wrapping_add(1);
        if raw::state_set(&[new_votes], &vck) < 1 {
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

/// First-ever `Invoke` on this table: reads `IMC` (+`IRR`/`IRD` on L1)
/// and `IS0..IS{imc-1}` hook parameters and populates the initial seat
/// table. Diverges (`accept!`/`rollback!`) — govern.c's setup path never
/// falls through to normal voting.
#[inline(never)]
fn setup(is_l1_table: bool) -> ! {
    let mut imc = [0u8; 1];
    if raw::hook_param(&mut imc, b"IMC") < 0 {
        GovernError::BadParameter
            .nope(b"Governance: Initial Member Count Parameter missing (IMC).");
    }
    if raw::state_set(&imc, &keys::MEMBER_COUNT) < 1 {
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
        let mut irr = [0u8; 8];
        if raw::hook_param(&mut irr, b"IRR") < 0 {
            GovernError::BadParameter
                .nope(b"Governance: Initial Reward Rate Parameter missing (IRR).");
        }
        let mut ird = [0u8; 8];
        if raw::hook_param(&mut ird, b"IRD") < 0 {
            GovernError::BadParameter
                .nope(b"Governance: Initial Reward Delay Parameter miss (IRD).");
        }
        if ird == [0u8; 8] {
            GovernError::BadParameter.nope(b"Governance: Initial Reward Delay must be > 0.");
        }
        if raw::state_set(&irr, &keys::REWARD_RATE) < 1 {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
        if raw::state_set(&ird, &keys::REWARD_DELAY) < 1 {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
    }

    let mut i = 0u8;
    while i < member_count {
        guard!(u32::from(SEAT_COUNT));
        let this_seat = i;
        i = i.wrapping_add(1);
        let member_pkey = [b'I', b'S', this_seat];
        let mut member_acc = AccountId::zeroed();
        if raw::hook_param(member_acc.as_mut(), &member_pkey) != 20 {
            GovernError::BadParameter
                .nope(b"Governance: One or more initial member account ID's is missing");
        }
        if raw::state_set(member_acc.as_ref(), &keys::seat_forward_key(this_seat)) != 20 {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
        if raw::state_set(&[this_seat], &keys::member_reverse_key(&member_acc)) != 1 {
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
    if raw::state_set(value, &key) < 1 {
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
    let keylet = take_scratch(&HOOK_KEYLET);
    if raw::hook_keylet(keylet, hook_accid) < 0 {
        GovernError::AssertionFailed.nope(b"govern: could not build hook keylet");
    }
    if raw::slot_set(keylet, 5) == 5 && raw::slot_subfield(5, sfHooks, 6) == 6 {
        let existing = raw::slot_subarray(6, u32::from(n), 7);
        if existing == 7 && raw::slot_subfield(7, sfHookHash, 8) == 8 {
            let existing_hook = take_scratch(&EXISTING_HOOK);
            if raw::slot(existing_hook, 8) != 32 {
                GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
            }
            if buf_eq_32(existing_hook, topic_data) {
                done(b"Goverance: Target hook is already the same as actioned hook.");
            }
        }
    }

    if !topic_data_zero {
        let hdef_keylet = take_scratch(&HOOK_DEFINITION_KEYLET);
        if raw::hook_definition_keylet(hdef_keylet, topic_data) < 0 {
            GovernError::AssertionFailed.nope(b"govern: could not build hook definition keylet");
        }
        if raw::slot_set(hdef_keylet, 9) != 9 {
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
        raw::state(dst, &keys::seat_forward_key(n)) == 20
    };

    if previous_present {
        let Some(prev_account) = previous_member.get(12..32) else {
            GovernError::AssertionFailed.nope(b"govern: bad buffer");
        };
        if bytes_eq(prev_account, new_account) {
            done(b"Governance: Actioning seat change, but seat already contains the new member.");
        }
    }

    let existing_member = raw::state_i64(new_account);
    let existing_member_moving = existing_member >= 0;

    let op: u8 = (u8::from(!previous_present) << 2)
        | (u8::from(topic_data_zero) << 1)
        | u8::from(existing_member_moving);
    if op == 0b011 || op == 0b111 {
        GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
    }

    let mut member_count = raw::state_i64(&keys::MEMBER_COUNT);
    if op == 0b001 || op == 0b010 {
        member_count = member_count.wrapping_sub(1);
    } else if op == 0b100 {
        member_count = member_count.wrapping_add(1);
    }
    if member_count <= 1 {
        GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
    }
    let mc = member_count as u8;
    if raw::state_set(&[mc], &keys::MEMBER_COUNT) != 1 {
        GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
    }

    if existing_member_moving {
        let m = existing_member as u8;
        if raw::state_delete(&keys::seat_forward_key(m)) != 0 {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
        if raw::state_delete(new_account) != 0 {
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

        if raw::state_delete(&keys::seat_forward_key(n)) != 0 {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
        let Some(prev_account) = previous_member.get(12..32) else {
            GovernError::AssertionFailed.nope(b"govern: bad buffer");
        };
        if raw::state_delete(prev_account) != 0 {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
    }

    if !topic_data_zero {
        if raw::state_set(new_account, &keys::seat_forward_key(n)) != 20 {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
        if raw::state_set(&[n], new_account) != 1 {
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
            raw::state(dst, previous_member.as_ref())
        };
        if read != topic_size as i64 {
            continue;
        }

        let Some(value) = vote_value.get(padding..) else {
            continue;
        };
        let vck = keys::vote_count_key(topic_type, topic_id, tbl, value);
        let mut vote_count = [0u8; 1];
        if raw::state(&mut vote_count, &vck) == 1 {
            if vote_count[0] <= 1 {
                let _ = raw::state_delete(&vck);
            } else {
                let dec = vote_count[0].wrapping_sub(1);
                let _ = raw::state_set(&[dec], &vck);
            }
        }
        let _ = raw::state_delete(previous_member.as_ref());
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
