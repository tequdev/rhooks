//! Integration coverage for `hook_state!`/`hook_parameter!`/
//! `otxn_parameter!`'s **entity handles**: the accessors they carry, the
//! role traits they implement, the forms that attach to caller-owned types
//! (`existing`, pairing), and `state_delete`.
//!
//! An integration test (`tests/`), not an in-crate `#[cfg(test)]` module,
//! because every one of those expansions is fully path-qualified against
//! `::rshooks` — a path that does not resolve from *inside* `rshooks`
//! itself (same reason `state.rs`'s own doctests are doctests).
//!
//! # What a host build can and cannot prove here
//!
//! Every Hook API call in this crate resolves to `rshooks-core`'s host stub
//! on a non-wasm target, and *every* stub returns the same
//! `NOT_IMPLEMENTED`. That bounds what these tests can claim. They prove
//! **typechecking and reachability**: that a declaration compiles, that the
//! entity is constructible and usable, that each generated method exists
//! with the signature the design specifies, and that each resolves its
//! value type from the declaration itself with no turbofish (pinned by the
//! explicit `Result<T>` annotations below, not by `.err()` comparisons —
//! those erase the success type).
//!
//! They do **not** prove which host call was selected, because every
//! candidate returns the same value: an `otxn_parameter!` entity reaching
//! `otxn_param` rather than `hook_param` is indistinguishable here, and so
//! is `delete_state` passing an empty value rather than any other. Those
//! are host *behavior*, covered live: `e2e/test/typed-data.test.ts`
//! ("deletes the state entry on a full withdrawal") deposits, withdraws,
//! and then asserts the entry is absent from the account's namespace
//! directory over RPC — the only place the delete contract is actually
//! observable.
//!
//! This file is also part of the lint gate: `mise run lint` runs clippy
//! with `--all-targets -D warnings`, so the deliberately-unused entities
//! and never-called methods below fail the build if the generated
//! `#[allow(dead_code)]` allowances ever stop covering them.

use rshooks::prelude::*;
use rshooks::{
    HookData, HookKey, ParamName, ParamValue, hook_parameter, hook_state, otxn_parameter,
    state_keys,
};

/// Every host stub returns this. Seeing it proves the call typechecked and
/// reached *a* host stub — which stub (hook-vs-otxn routing, empty-value
/// passing) is exactly what stubs cannot distinguish; see the module doc.
const STUB: HookError = HookError::NotImplemented;

// ---------------------------------------------------------------------------
// The declaring forms (1–4)
// ---------------------------------------------------------------------------

// Form 1, all three macros.
hook_state!(RewardRate, RewardRateKey = b"RR" => u64);
hook_parameter!(Cfg, CfgName = b"CFG" => Config {min_amount: u64});
otxn_parameter!(Ins, InsName = b"INS" => Instruction {action: u8});

// Form 2: the fixed instance is a `const` of the *entity's* name.
hook_state!(Counter, CounterKey {name: [u8; 7]} = {name: *b"counter"} => u64);

// Form 3: constructed per call site.
hook_state!(DepositState, DepositKey {tag: u8, owner: AccountId} => DepositValue {amount: u64, deadline: u32});

// Form 4: newtype around one existing type.
hook_state!(AccountState, AccountKey AccountId => u64);
hook_parameter!(MemberParam, MemberParamName [u8; 3] => Config);
otxn_parameter!(TopicParam, TopicParamName [u8; 2] => Instruction);

// A declaration whose accessors are *never* called — the per-method
// `#[allow(dead_code)]` is what keeps this lint-clean.
hook_state!(NeverCalled, NeverCalledKey = b"NC" => u64);

#[test]
fn form1_entity_carries_the_accessors() {
    assert_eq!(RewardRate.get_state().err(), Some(STUB));
    assert_eq!(RewardRate.set_state(&1u64), Err(STUB));
    assert_eq!(
        RewardRate.update_state(|current| current.unwrap_or(0)),
        Err(STUB)
    );
    assert_eq!(RewardRate.delete_state(), Err(STUB));

    assert_eq!(Cfg.get_name(), b"CFG".as_slice());
    assert_eq!(Cfg.get_value().err(), Some(STUB));
    assert_eq!(Ins.get_name(), b"INS".as_slice());
    assert_eq!(Ins.get_value().err(), Some(STUB));

    // Value types resolved from the declaration, not from the binding.
    let _: Result<Option<u64>> = RewardRate.get_state();
    let _: Result<Config> = Cfg.get_value();
    let _: Result<Instruction> = Ins.get_value();
    let _: &'static [u8] = Cfg.get_name();
}

#[test]
fn form2_declares_the_const_on_the_entity() {
    // `Counter` is both a type and the one `const` instance of it.
    assert_eq!(Counter.name, *b"counter");
    assert_eq!(Counter.get_state().err(), Some(STUB));

    // The key component encodes identically and is still usable directly.
    assert_eq!(
        state_get_typed(&CounterKey { name: *b"counter" }).err(),
        Some(STUB)
    );
}

#[test]
fn form3_entity_mirrors_the_key_fields() {
    let owner = AccountId::default();
    let deposit = DepositState { tag: 1, owner };

    assert_eq!(deposit.get_state().err(), Some(STUB));
    assert_eq!(
        deposit.set_state(&DepositValue {
            amount: 1,
            deadline: 2,
        }),
        Err(STUB)
    );
    let _: Result<Option<DepositValue>> = deposit.get_state();

    // The key type carries the same encoding and none of the methods.
    assert_eq!(
        state_get_typed(&DepositKey { tag: 1, owner }).err(),
        Some(STUB)
    );
}

#[test]
fn form4_newtype_entity_carries_the_accessors() {
    let account = AccountState(AccountId::default());
    assert_eq!(account.get_state().err(), Some(STUB));
    assert_eq!(account.set_state(&1u64), Err(STUB));
    assert_eq!(account.update_state(|c| c.unwrap_or(0)), Err(STUB));
    assert_eq!(account.delete_state(), Err(STUB));
    let _: Result<Option<u64>> = account.get_state();

    let member = MemberParam(*b"IS0");
    assert_eq!(member.get_value().err(), Some(STUB));
    let _: Result<Config> = member.get_value();

    let topic = TopicParam(*b"TT");
    assert_eq!(topic.get_value().err(), Some(STUB));
    let _: Result<Instruction> = topic.get_value();
}

// ---------------------------------------------------------------------------
// The forms that attach to caller-owned types
// ---------------------------------------------------------------------------

/// A caller-declared marker type carrying its own visibility and docs — the
/// reason to declare a key type yourself rather than let the macro do it.
pub struct OwnStateKey;
hook_state!(OwnState, existing OwnStateKey = b"OSK" => u64);

/// A value read by both `existing`-form parameter entities below. `pub`
/// because the caller-owned name types are: `existing` gives *those* types
/// the `TypedParamName` impl, whose `type Value` would otherwise leak a
/// private type out of a public one (`E0446`).
#[derive(ParamValue)]
pub struct OwnValue {
    min_amount: u64,
}

/// Caller-declared name marker for the `hook_parameter!` `existing` form.
pub struct OwnHookName;
hook_parameter!(OwnHookParam, existing OwnHookName = b"OHN" => OwnValue);

/// Caller-declared name marker for the `otxn_parameter!` `existing` form.
pub struct OwnOtxnName;
otxn_parameter!(OwnOtxnParam, existing OwnOtxnName = b"OON" => OwnValue);

/// A caller-owned key type this macro could not construct even if it wanted
/// to: no public constructor, no `Copy`, no `Default`. The `existing` form
/// still works, which is the point.
pub struct Unconstructible {
    _private: (),
}
hook_state!(UnconstructibleState, existing Unconstructible = b"UC" => u64);

#[test]
fn existing_form_attaches_impls_without_constructing_the_type() {
    assert_eq!(OwnState.get_state().err(), Some(STUB));
    assert_eq!(OwnState.set_state(&1u64), Err(STUB));
    assert_eq!(OwnState.delete_state(), Err(STUB));
    assert_eq!(state_get_typed(&OwnStateKey).err(), Some(STUB));

    assert_eq!(OwnHookParam.get_name(), b"OHN".as_slice());
    assert_eq!(OwnHookParam.get_value().err(), Some(STUB));
    assert_eq!(hook_param_typed(&OwnHookName).err(), Some(STUB));

    assert_eq!(OwnOtxnParam.get_name(), b"OON".as_slice());
    assert_eq!(OwnOtxnParam.get_value().err(), Some(STUB));
    assert_eq!(otxn_param_typed(&OwnOtxnName).err(), Some(STUB));

    // The entity works even though nothing can build the key it names.
    assert_eq!(UnconstructibleState.get_state().err(), Some(STUB));

    let _: Result<OwnValue> = OwnHookParam.get_value();
    let _: Result<OwnValue> = OwnOtxnParam.get_value();

    // Hand-written value structs (unlike macro-declared ones, which carry
    // their own `#[allow(dead_code)]`) need their fields genuinely read.
    assert_eq!(OwnValue { min_amount: 5 }.min_amount, 5);
}

/// `get_name` is `const fn`, so a fixed name's bytes are usable wherever a
/// constant is — no runtime encode, nothing to inline away.
const OWN_HOOK_NAME_BYTES: &[u8] = OwnHookParam.get_name();

#[test]
fn get_name_is_usable_in_const_context() {
    assert_eq!(OWN_HOOK_NAME_BYTES, b"OHN".as_slice());
}

// --- pairing: a `#[derive(HookKey)]` struct --------------------------------

#[derive(HookKey, Clone, Copy)]
struct PairedKey {
    tag: u8,
}

#[derive(HookData, Clone, Copy, Debug, PartialEq)]
struct PairedValue {
    count: u32,
}

hook_state!(PairedState, PairedKey => PairedValue);

// --- pairing: a `state_keys!` enum -----------------------------------------
//
// The case a `#[derive(HookKey)]`-only fixture would miss: a `state_keys!`
// enum has `StateKeyEncode` but **no** `ToBytes`, so the entity's forwarding
// has to go through `StateKeyEncode::encode` rather than re-encoding fields.

state_keys! {
    /// This hook's persistent data.
    enum DataKey {
        /// A running counter.
        Counter,
        /// A per-owner balance, keyed by the owner's account.
        Balance(AccountId),
    }
}

hook_state!(DataState, DataKey => u64);

// --- pairing: a non-`Copy` key ---------------------------------------------
//
// Nothing generated copies or constructs the key, so `Copy` is not required
// anywhere — the entity borrows the one the caller put in it.

#[derive(HookKey)]
struct NonCopyKey {
    tag: u8,
    owner: AccountId,
}

hook_state!(NonCopyState, NonCopyKey => u64);

// --- pairing: a parameter name ---------------------------------------------

#[derive(ParamName, Clone, Copy)]
struct SeatName {
    topic: u8,
    seat: u8,
}

#[derive(ParamValue)]
struct SeatVote {
    value: u8,
}

hook_parameter!(SeatParam, SeatName => SeatVote);

#[test]
fn pairing_entity_wraps_and_forwards() {
    assert_eq!(
        PairedState(PairedKey { tag: 0 }).get_state().err(),
        Some(STUB)
    );
    assert_eq!(
        PairedState(PairedKey { tag: 0 }).set_state(&PairedValue { count: 1 }),
        Err(STUB)
    );
    let _: Result<Option<PairedValue>> = PairedState(PairedKey { tag: 0 }).get_state();

    // A `state_keys!` enum: `StateKeyEncode` without `ToBytes`.
    assert_eq!(DataState(DataKey::Counter).get_state().err(), Some(STUB));
    assert_eq!(
        DataState(DataKey::Balance(AccountId::default()))
            .get_state()
            .err(),
        Some(STUB)
    );
    let _: Result<Option<u64>> = DataState(DataKey::Counter).get_state();

    // A non-`Copy` key, moved into the entity once and borrowed after.
    let non_copy = NonCopyState(NonCopyKey {
        tag: 1,
        owner: AccountId::default(),
    });
    assert_eq!(non_copy.get_state().err(), Some(STUB));
    assert_eq!(non_copy.delete_state(), Err(STUB));

    // Parameter side: the entity forwards `with_name_bytes` to the name's
    // own implementation rather than re-deriving one.
    let seat = SeatParam(SeatName {
        topic: b'S',
        seat: 0,
    });
    assert_eq!(seat.get_value().err(), Some(STUB));
    let _: Result<SeatVote> = seat.get_value();
    assert_eq!(SeatVote { value: 1 }.value, 1);

    // The paired key/name types keep working with the free functions.
    assert_eq!(state_get_typed(&PairedKey { tag: 0 }).err(), Some(STUB));
    assert_eq!(state_get_typed(&DataKey::Counter).err(), Some(STUB));
}

// ---------------------------------------------------------------------------
// The entity is a first-class key/name, not just a method holder
// ---------------------------------------------------------------------------

#[test]
fn entity_reaches_the_free_loose_and_foreign_apis() {
    let deposit = DepositState {
        tag: 1,
        owner: AccountId::default(),
    };

    // Typed free functions.
    assert_eq!(state_get_typed(&deposit).err(), Some(STUB));
    assert_eq!(
        state_set_typed(
            &deposit,
            &DepositValue {
                amount: 1,
                deadline: 2
            }
        ),
        Err(STUB)
    );

    // Loose free functions (independent value type).
    assert_eq!(state_get::<u64>(&deposit).err(), Some(STUB));

    // `_foreign` twins — the whole reason the entity implements the role
    // traits instead of only carrying methods.
    assert_eq!(
        state_foreign_get_typed(&deposit, None, None).err(),
        Some(STUB)
    );
    assert_eq!(
        state_foreign_set_typed(
            &deposit,
            &DepositValue {
                amount: 1,
                deadline: 2
            },
            None,
            None
        ),
        Err(STUB)
    );

    // Deletion takes any `StateKeyEncode`, entity included.
    assert_eq!(state_delete(&deposit), Err(STUB));
    assert_eq!(state_delete(&RewardRate), Err(STUB));
}

// ---------------------------------------------------------------------------
// Hygiene: a declaration routed through a caller's own `macro_rules!`
// ---------------------------------------------------------------------------

/// A wrapper macro of the kind a hook crate writes to stamp out several
/// similar declarations, forwarding the entity, key and value identifiers
/// *and* a field type — every ident the grammar takes.
macro_rules! declare_wrapped {
    ($entity:ident, $key:ident, $value:ident, $field_ty:ty) => {
        hook_state!($entity, $key {tag: u8, owner: $field_ty} => $value {seen: u64});
    };
}

#[test]
fn wrapper_macro_rules_forwards_every_identifier() {
    declare_wrapped!(WrappedState, WrappedKey, WrappedValue, AccountId);

    let wrapped = WrappedState {
        tag: 1,
        owner: AccountId::default(),
    };
    assert_eq!(wrapped.get_state().err(), Some(STUB));
    let _: Result<Option<WrappedValue>> = wrapped.get_state();
}

// ---------------------------------------------------------------------------
// `state_delete`
// ---------------------------------------------------------------------------

#[test]
fn state_delete_is_reachable_as_a_free_function_and_as_a_method() {
    // Routing only: the host stub cannot demonstrate that an empty write
    // deletes the entry (see this file's module doc comment).
    assert_eq!(state_delete(&RewardRateKey), Err(STUB));
    assert_eq!(RewardRate.delete_state(), Err(STUB));
}

#[test]
fn state_delete_accepts_any_state_key_encode_key() {
    // No `TypedStateKey` pairing needed: deletion has no value type.
    assert_eq!(state_delete(&[0u8; 4]), Err(STUB));
}

// ---------------------------------------------------------------------------
// Lint gate: unused entities, unused methods
// ---------------------------------------------------------------------------

// An unused *composite* entity at module scope: its mirrored fields are
// referenced only from `#[automatically_derived]` impls, which is exactly
// the shape that still warns without `#[allow(dead_code)]` on the struct.
hook_state!(UnusedComposite, UnusedCompositeKey {tag: u8, owner: AccountId} => UnusedCompositeValue {amount: u64});

// An unused Form 2 declaration: the entity `const` needs its own
// `#[allow(dead_code, non_upper_case_globals)]`.
hook_state!(UnusedFixedInstance, UnusedFixedKey {tag: u8} = {tag: 3} => u64);

#[test]
fn unused_entities_are_lint_clean() {
    // Nothing above or below is ever read, and none of the methods any of
    // them brings along is ever called. Under `mise run lint`
    // (`--all-targets -D warnings`) this compiles only because the
    // expansion carries `#[allow(dead_code)]` on every declared struct, on
    // the Form 2 `const`, and on every generated method.
    hook_state!(UnusedState, UnusedStateKey = b"US" => u64);
    hook_parameter!(UnusedHookParam, UnusedHookName = b"UH" => Config);
    otxn_parameter!(UnusedOtxnParam, UnusedOtxnName = b"UO" => Config);
    hook_state!(UnusedLocalComposite, UnusedLocalKey {tag: u8} => u64);
}
