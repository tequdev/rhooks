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
//! Every Hook API call in this crate goes through `hooks_lib`'s ordinary
//! `Result`-based wrappers (`otxn_field_exact`, `hook_account_buf`,
//! `hook_param`, `otxn_param`, `state`, `state_u64`, `state_set`,
//! `slot_subfield`, `util_keylet`, `emit_buf`, ...) — this crate has no
//! `raw` module at all, unlike `examples/80_reward` (which keeps one,
//! narrowly, for `float_*`/`slot_float` — see that crate's `src/raw.rs`
//! doc comment for why XFL specifically is different). govern.c has no
//! XFL arithmetic of its own to port, so there is nothing here that needs
//! the same treatment.
//!
//! An earlier version of this crate *did* route every Hook API call
//! through a `raw` module, on the theory that `hooks_lib::error::res`'s
//! `HookError::from(i64)` decode (see `examples/80_reward/src/raw.rs`'s
//! doc comment) would otherwise push `hook()`'s block/loop/if nesting
//! past the Hook API's 32-level limit once every function in the crate
//! is inlined into it. That turned out to be broader than necessary:
//! with the *structural* mitigations below in place — none of which
//! involve bypassing `hooks_lib::api` — the ordinary `Result`-based
//! wrappers fit comfortably (see the measurements at the end of this
//! comment). The structural mitigations, kept:
//!
//! - a per-package `opt-level = 3` override for this crate specifically
//!   (`examples/Cargo.toml`) — `opt-level = "z"` (every other example's
//!   default) leaves more dead error-decoding code in place;
//! - [`txn`]'s transaction encoders build each emitted transaction from a
//!   handful of large, combined `push` calls (concatenating adjacent
//!   constant/semi-constant byte fragments) instead of one call per
//!   field, and skip placeholder-then-patch round trips for fields
//!   already known at write time — see `txn.rs`'s `write_common_header`;
//! - `HookStatic` scratch buffers (below) instead of stack locals for
//!   every 32-byte-and-up buffer, and `#[inline(always)]` on the small
//!   hot helpers that touch them;
//! - loops are written as manual `while`s (guard call literally first)
//!   rather than `for x in range`, and a `guard!` bound that must hold
//!   for a whole hook execution (not just one loop entry) uses the wider
//!   bound even when the loop's own body would justify a tighter one —
//!   see [`garbage_collect_votes`]'s doc comment;
//! - exactly one `Result` is ever matched against a *specific*
//!   `HookError` variant in this whole crate ([`my_hook`]'s
//!   `member_count` read) — see the comment right there for why that
//!   one call site, and only that one, has to avoid it.
//!
//! Measured (`hooks-build check examples/81_govern/out/govern.wasm`):
//! worst-case instructions 44436, max nesting depth 22 (limit 32) — see
//! the README's "Toolchain limitation" section for the fuller writeup,
//! including the live `GUARD_VIOLATION` the guard-bound point above
//! fixes.
//!
//! Build: `hooks-build build --manifest-path examples/81_govern/Cargo.toml`

#![no_std]

mod keys;
mod txn;

use hooks_lib::prelude::*;
use hooks_lib::static_cell::HookStatic;
use hooks_lib::{accept, guard, hook, hook_errors, rollback};

// `IS{seat}` — the initial-member-account hook parameter name `setup`
// reads per seat (`this_seat` runtime-varying, 0..SEAT_COUNT — see `setup`
// below).
//
// A first attempt at typing this name used `hook_parameter!`'s Form 4 (a
// newtype wrapping `[u8; 3]`, the same shape the raw array literal already
// was): `hook_parameter!(MemberParamName [u8; 3] => AccountId);`, storing
// the full 3-byte name and relying on `TypedParamName::name_bytes`'s
// *default* body. That body is generic over any `ToBytes` name — it must
// call `self.write(buf)` (a real per-call encode into a 32-byte stack
// scratch buffer: a runtime-bounds-checked `buf.get_mut(..3)` slice, a
// 3-byte `copy_from_slice`, then a second `buf.get(..3)` to hand the
// written prefix back out) — where the raw `hook_param_exact(&member_pkey)`
// this replaced just passed `member_pkey`'s own `&[u8; 3]` straight
// through as a pointer+length, no copy, no bounds check, no buffer at
// all. Measured: +607 worst-case instructions (44560 -> 45167) for that
// copy-through-a-buffer indirection, paid once per seat in the setup
// loop's worst case (`guard!(SEAT_COUNT)`, i.e. `hooks-build`'s
// worst-case-instruction accounting charges the loop body's cost
// `SEAT_COUNT` times over) — nesting depth was unaffected (22, unchanged;
// `hook_param_typed`/`hook_param_exact` never match a specific `HookError`
// variant, so none of Task A's fix was relevant here). Reverted (the raw
// baseline, before either attempt: 44560 worst-case instructions, 14418
// bytes, nesting 22).
//
// The actual fix needed a *zero-copy* `name_bytes` — the same "return a
// `'static` slice directly, no buffer, nothing to encode" shape
// `hook_parameter!`'s plain-byte-string forms already get (see
// `hooks_lib::convert::TypedParamName`'s "Zero-cost" doc section) — but
// for a name that is one of several fixed byte strings, runtime-selected,
// not a single fixed one. Since there are only `SEAT_COUNT` (20) possible
// names, `SEAT_PARAM_NAMES` below is a `const`-evaluated lookup table of
// all 20 (`[b'I', b'S', 0]` .. `[b'I', b'S', 19]`, precisely the bytes
// `setup` used to build inline), and `MemberParamName` is redefined here
// (hand-written, not through `hook_parameter!` — no grammar form
// generates this shape) to wrap just the seat number and override
// `name_bytes` to index straight into the table, handing back a
// `'static` reference with no buffer and no copy — via `.get(..)`, not a
// literal/computed index (`SEAT_PARAM_NAMES[..]`), so no
// `clippy::indexing_slicing`/`arithmetic_side_effects` allow is needed:
// `self.0` is already always `< SEAT_COUNT` by construction (see `setup`'s
// loop bound), so `FALLBACK_NAME` is never actually reached, but its
// presence lets the lookup stay a safe, panic-free `Option` chain rather
// than an indexing expression clippy can't itself prove in-bounds.
// Measured: worst-case instructions 44436 — 124 *below* the pre-migration
// raw baseline (44560): the LUT lookup (an `Option`-returning `.get(..)`
// against a fixed, already-laid-out 60-byte const table) turned out
// slightly cheaper than the raw path's per-iteration `[b'I', b'S',
// this_seat]` array-literal construction, since there is no longer
// anything to *build* per seat, only to index. Nesting depth unchanged
// (22); size 14426 bytes (14418 raw baseline + 8, the const table minus
// what the no-longer-built stack array cost). Byte-for-byte identical
// parameter names to both the raw baseline and govern.c.
#[allow(clippy::indexing_slicing)] // `i < names.len()` is the loop condition itself: every index here is proven in-bounds by construction; `slice::get_mut` isn't usable in a `const fn` at this crate's MSRV.
const fn build_seat_param_names() -> [[u8; 3]; SEAT_COUNT as usize] {
    let mut names = [[0u8; 3]; SEAT_COUNT as usize];
    let mut i = 0usize;
    while i < names.len() {
        names[i] = [b'I', b'S', i as u8];
        i = i.wrapping_add(1);
    }
    names
}
const SEAT_PARAM_NAMES: [[u8; 3]; SEAT_COUNT as usize] = build_seat_param_names();
/// Never actually returned — every `self.0` this crate constructs is
/// already `< SEAT_COUNT` (see `setup`'s loop bound) — but gives
/// [`SEAT_PARAM_NAMES`]'s `.get(..).unwrap_or(..)` lookups below a
/// `'static` fallback so they stay a safe `Option` chain instead of a
/// literal/computed index.
const FALLBACK_NAME: [u8; 3] = [0, 0, 0];

/// Wraps a seat number (`0..SEAT_COUNT`), not the 3-byte name itself — see
/// the doc comment above for why. `ToBytes` is still required
/// ([`TypedParamName`]'s supertrait bound), and kept behaviorally
/// consistent with the [`TypedParamName::name_bytes`] override below (same
/// [`SEAT_PARAM_NAMES`] lookup), even though `hook_param_typed` never
/// calls it — `name_bytes` is the only path actually exercised.
struct MemberParamName(u8);

#[automatically_derived]
impl ::hooks_lib::convert::ToBytes for MemberParamName {
    const MAX_LEN: usize = 3;

    #[inline(always)]
    fn write(&self, buf: &mut [u8]) -> usize {
        match buf.get_mut(..3) {
            Some(dst) => {
                let name = SEAT_PARAM_NAMES
                    .get(usize::from(self.0))
                    .unwrap_or(&FALLBACK_NAME);
                dst.copy_from_slice(name);
                3
            }
            None => 0,
        }
    }
}

#[automatically_derived]
impl ::hooks_lib::convert::TypedParamName for MemberParamName {
    type Value = AccountId;

    #[inline(always)]
    fn name_bytes<'buf>(
        &self,
        _buf: &'buf mut [u8; ::hooks_lib::convert::PARAM_NAME_MAX_LEN],
    ) -> &'buf [u8] {
        SEAT_PARAM_NAMES
            .get(usize::from(self.0))
            .unwrap_or(&FALLBACK_NAME)
    }
}

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
// `ACCOUNT_KEYLET` for the same fix there) — unrelated to which API wraps
// the calls that fill them.
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
    if etxn_reserve(1).is_err() {
        GovernError::EmitFailed.nope(b"govern: etxn_reserve failed");
    }

    if otxn_type() != TxType::Invoke {
        done(b"Governance: Passing non-Invoke txn. HookOn should be changed to avoid this.");
    }

    let sender: AccountId = match otxn_field_exact(sfAccount) {
        Ok(a) => a,
        Err(_) => GovernError::AssertionFailed.nope(b"govern: could not read otxn Account"),
    };
    let hook_accid: AccountId = match hook_account_buf() {
        Ok(a) => a,
        Err(_) => GovernError::AssertionFailed.nope(b"govern: could not read hook_account"),
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

    let mut topic = [0u8; 2];
    let topic_ok = otxn_param(&mut topic, b"T") == Ok(2);
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
        let mut lbuf = [0u8; 1];
        if otxn_param(&mut lbuf, b"L") != Ok(1) {
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

/// First-ever `Invoke` on this table: reads `IMC` (+`IRR`/`IRD` on L1)
/// and `IS0..IS{imc-1}` hook parameters and populates the initial seat
/// table. Diverges (`accept!`/`rollback!`) — govern.c's setup path never
/// falls through to normal voting.
#[inline(never)]
fn setup(is_l1_table: bool) -> ! {
    let mut imc = [0u8; 1];
    if hook_param(&mut imc, b"IMC").is_err() {
        GovernError::BadParameter
            .nope(b"Governance: Initial Member Count Parameter missing (IMC).");
    }
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
        let mut irr = [0u8; 8];
        if hook_param(&mut irr, b"IRR").is_err() {
            GovernError::BadParameter
                .nope(b"Governance: Initial Reward Rate Parameter missing (IRR).");
        }
        let mut ird = [0u8; 8];
        if hook_param(&mut ird, b"IRD").is_err() {
            GovernError::BadParameter
                .nope(b"Governance: Initial Reward Delay Parameter miss (IRD).");
        }
        if ird == [0u8; 8] {
            GovernError::BadParameter.nope(b"Governance: Initial Reward Delay must be > 0.");
        }
        if state_set(&irr, &keys::REWARD_RATE).is_err() {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
        if state_set(&ird, &keys::REWARD_DELAY).is_err() {
            GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
        }
    }

    let mut i = 0u8;
    while i < member_count {
        guard!(u32::from(SEAT_COUNT));
        let this_seat = i;
        i = i.wrapping_add(1);
        let member_pkey = MemberParamName(this_seat);
        let member_acc: AccountId = match hook_param_typed(&member_pkey) {
            Ok(a) => a,
            Err(_) => GovernError::BadParameter
                .nope(b"Governance: One or more initial member account ID's is missing"),
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
    let keylet = take_scratch(&HOOK_KEYLET);
    if util_keylet(
        keylet,
        KEYLET_HOOK,
        hook_accid.as_ptr() as u32,
        ACC_ID_LEN as u32,
        0,
        0,
        0,
        0,
    )
    .is_err()
    {
        GovernError::AssertionFailed.nope(b"govern: could not build hook keylet");
    }
    if slot_set(keylet, 5) == Ok(5) && slot_subfield(5, sfHooks, 6) == Ok(6) {
        let existing = slot_subarray(6, u32::from(n), 7);
        if existing == Ok(7) && slot_subfield(7, sfHookHash, 8) == Ok(8) {
            let existing_hook = take_scratch(&EXISTING_HOOK);
            if slot(existing_hook, 8) != Ok(32) {
                GovernError::AssertionFailed.nope(b"Governance: Assertion failed.");
            }
            if buf_eq_32(existing_hook, topic_data) {
                done(b"Goverance: Target hook is already the same as actioned hook.");
            }
        }
    }

    if !topic_data_zero {
        let hdef_keylet = take_scratch(&HOOK_DEFINITION_KEYLET);
        if util_keylet(
            hdef_keylet,
            KEYLET_HOOK_DEFINITION,
            topic_data.as_ptr() as u32,
            32,
            0,
            0,
            0,
            0,
        )
        .is_err()
        {
            GovernError::AssertionFailed.nope(b"govern: could not build hook definition keylet");
        }
        if slot_set(hdef_keylet, 9) != Ok(9) {
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
