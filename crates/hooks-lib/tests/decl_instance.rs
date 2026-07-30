//! Integration coverage for `hook_state!`/`hook_parameter!`/
//! `otxn_parameter!`'s **instance binder**, their generated **inherent
//! accessors**, the `existing` keyword form, and `state_delete`.
//!
//! An integration test (`tests/`), not an in-crate `#[cfg(test)]` module,
//! because every one of those expansions is fully path-qualified against
//! `::hooks_lib` — a path that does not resolve from *inside* `hooks_lib`
//! itself (same reason `state.rs`'s own doctests are doctests).
//!
//! # What a host build can and cannot prove here
//!
//! Every Hook API call in this crate resolves to `hooks-core`'s host stub
//! on a non-wasm target, and *every* stub returns the same
//! `NOT_IMPLEMENTED`. That bounds what these tests can claim. They prove
//! **typechecking and reachability**: that a declaration compiles, that its
//! binder binds a usable instance, that each generated method exists with
//! the signature the design specifies, and that each resolves its value
//! type from the declaration itself with no turbofish (pinned by the
//! explicit `Result<T>` annotations below, not by `.err()` comparisons —
//! those erase the success type).
//!
//! They do **not** prove which host call was selected, because every
//! candidate returns the same value: an `otxn_parameter!` name reaching
//! `otxn_param` rather than `hook_param` is indistinguishable here, and so
//! is `delete_state` passing an empty value rather than any other. Those
//! are host *behavior*, covered live: `e2e/test/typed-data.test.ts`
//! ("deletes the state entry on a full withdrawal") deposits, withdraws,
//! and then asserts the entry is absent from the account's namespace
//! directory over RPC — the only place the delete contract is actually
//! observable.
//!
//! This file is also part of the lint gate: `mise run lint` runs clippy
//! with `--all-targets -D warnings`, so the deliberately-unused binders and
//! never-called methods below fail the build if the generated
//! `#[allow(unused_variables)]`/`#[allow(unused_mut)]`/`#[allow(dead_code)]`
//! attributes ever stop covering them.

use hooks_lib::prelude::*;
use hooks_lib::{ParamValue, hook_parameter, hook_state, otxn_parameter};

/// Every host stub returns this. Seeing it proves the call typechecked and
/// reached *a* host stub — which stub (hook-vs-otxn routing, empty-value
/// passing) is exactly what stubs cannot distinguish; see the module doc.
const STUB: HookError = HookError::NotImplemented;

// ---------------------------------------------------------------------------
// Module-scope declarations (the non-binder forms, unchanged by this feature)
// ---------------------------------------------------------------------------

// Form 2 at module scope: still declares the same-named `const`, and now
// also carries the four state accessors.
hook_state!(CounterKey {tag: u8} = {tag: 7} => u64);

// A declaration whose methods are *never* called — the per-method
// `#[allow(dead_code)]` is what keeps this lint-clean.
hook_state!(NeverCalledKey = b"NC" => u64);

/// A caller-declared marker type carrying its own visibility and docs — the
/// capability the removed `Name, b".." => Ty` comma-form used to provide.
pub struct OwnStateKey;
hook_state!(existing OwnStateKey = b"OSK" => u64);

/// A parameter value read by both `existing`-form names below.
#[derive(ParamValue)]
struct Config {
    min_amount: u64,
}

/// Caller-declared name marker for the `hook_parameter!` `existing` form.
struct OwnHookName;
hook_parameter!(existing OwnHookName = b"OHN" => Config);

/// Caller-declared name marker for the `otxn_parameter!` `existing` form.
struct OwnOtxnName;
otxn_parameter!(existing OwnOtxnName = b"OON" => Config);

// ---------------------------------------------------------------------------
// Instance binder: the three macros, Form 1
// ---------------------------------------------------------------------------

#[test]
fn state_form1_binder_binds_the_declared_instance() {
    hook_state!(counter, CounterStateKey = b"CTR" => u64);

    // The binder and the type's own name are the same (zero-sized) value,
    // so the method and free-function spellings must agree.
    assert_eq!(counter.get_state().err(), Some(STUB));
    assert_eq!(state_get_typed(&CounterStateKey).err(), Some(STUB));
    assert_eq!(counter.set_state(&1u64), Err(STUB));
    assert_eq!(state_set_typed(&CounterStateKey, &1u64), Err(STUB));
    assert_eq!(
        counter.update_state(|current| current.unwrap_or(0)),
        Err(STUB)
    );
    assert_eq!(counter.delete_state(), Err(STUB));
}

#[test]
fn hook_parameter_form1_binder_binds_the_declared_instance() {
    hook_parameter!(cfg, CfgParamName = b"CFG" => Config);

    assert_eq!(cfg.get_name(), b"CFG".as_slice());
    assert_eq!(cfg.get_value().err(), Some(STUB));
    assert_eq!(hook_param_typed(&CfgParamName).err(), Some(STUB));
}

#[test]
fn otxn_parameter_form1_binder_binds_the_declared_instance() {
    otxn_parameter!(ins, InsParamName = b"INS" => Config);

    assert_eq!(ins.get_name(), b"INS".as_slice());
    assert_eq!(ins.get_value().err(), Some(STUB));
    assert_eq!(otxn_param_typed(&InsParamName).err(), Some(STUB));
}

// ---------------------------------------------------------------------------
// Instance binder: the struct form (runtime initializer, `let mut`)
// ---------------------------------------------------------------------------

#[test]
fn struct_binder_takes_a_runtime_initializer_and_is_reassignable() {
    // Deliberately *not* a constant: the binder form's initializer is an
    // ordinary runtime expression (a `const` of the same name, which the
    // non-binder Form 2 declares, could not hold this).
    let owner = AccountId::default();
    let tag = 1u8 + 1;

    hook_state!(deposit, DepositKey {tag: u8, owner: AccountId} = {tag, owner}
                => DepositValue {amount: u64, deadline: u32});

    assert_eq!(deposit.tag, 2);
    assert_eq!(deposit.get_state().err(), Some(STUB));

    // `let mut`: the whole point of a struct binder is that the key's
    // fields can be re-aimed between accesses.
    deposit.tag = 3;
    assert_eq!(deposit.tag, 3);
    assert_eq!(state_get_typed(&deposit).err(), Some(STUB));
    assert_eq!(
        deposit.set_state(&DepositValue {
            amount: 1,
            deadline: 2,
        }),
        Err(STUB)
    );
}

#[test]
fn struct_binder_works_for_a_composite_parameter_name() {
    let section = 0u8;

    hook_parameter!(admin, AdminName {section: u8, field: u8} = {section, field: 0}
                    => PauseSwitch {paused: u8});

    assert_eq!(admin.get_value().err(), Some(STUB));
    assert_eq!(hook_param_typed(&admin).err(), Some(STUB));
}

// ---------------------------------------------------------------------------
// Hygiene: an initializer routed through a caller's own `macro_rules!`
// ---------------------------------------------------------------------------

/// A wrapper macro of the kind a hook crate writes to stamp out several
/// similar declarations. `$amount` arrives as an opaque `expr` fragment
/// carrying its *caller's* syntax context — the case that breaks if the
/// initializer is stringified and re-parsed instead of spliced verbatim.
macro_rules! declare_wrapped_key {
    ($binder:ident, $key:ident, $value:ident, $amount:expr) => {
        hook_state!($binder, $key {tag: u8, amount: u64} = {tag: 1, amount: $amount}
                    => $value {seen: u64});
    };
}

#[test]
fn wrapper_macro_rules_initializer_keeps_its_hygiene() {
    let local_amount: u64 = 7;
    declare_wrapped_key!(wrapped, WrappedKey, WrappedValue, local_amount);

    assert_eq!(wrapped.amount, 7);
    assert_eq!(wrapped.get_state().err(), Some(STUB));
}

// ---------------------------------------------------------------------------
// Module-scope forms: methods, `existing`, and the unchanged Form 2 `const`
// ---------------------------------------------------------------------------

#[test]
fn module_scope_declarations_carry_the_same_methods() {
    // Form 2's same-named `const` still exists, and now answers `.` with
    // the state accessors.
    assert_eq!(CounterKey.tag, 7);
    assert_eq!(CounterKey.get_state().err(), Some(STUB));
    assert_eq!(state_get_typed(&CounterKey).err(), Some(STUB));

    // Form 3 style: construct the key locally, call the method on it.
    assert_eq!(CounterKey { tag: 9 }.get_state().err(), Some(STUB));

    // A declaration whose methods are never called still has to compile
    // cleanly under `-D warnings`; touching the type (but none of its
    // methods) is what makes that meaningful.
    let never_called = NeverCalledKey;
    assert_eq!(state_get_typed(&never_called).err(), Some(STUB));
}

#[test]
fn existing_form_attaches_impls_and_methods_to_a_caller_declared_type() {
    // State: the `existing` form is the only way a caller-owned key type
    // gets fixed key bytes without this macro declaring it.
    assert_eq!(OwnStateKey.get_state().err(), Some(STUB));
    assert_eq!(OwnStateKey.set_state(&1u64), Err(STUB));
    assert_eq!(OwnStateKey.delete_state(), Err(STUB));
    assert_eq!(state_get_typed(&OwnStateKey).err(), Some(STUB));

    // Parameters: fixed name bytes come back from `get_name` at no cost.
    assert_eq!(OwnHookName.get_name(), b"OHN".as_slice());
    assert_eq!(OwnHookName.get_value().err(), Some(STUB));
    assert_eq!(hook_param_typed(&OwnHookName).err(), Some(STUB));

    assert_eq!(OwnOtxnName.get_name(), b"OON".as_slice());
    assert_eq!(OwnOtxnName.get_value().err(), Some(STUB));
    assert_eq!(otxn_param_typed(&OwnOtxnName).err(), Some(STUB));

    // Every assertion above goes through `.err()`, which throws the success
    // type away — so none of them actually pins what these methods return.
    // These annotations do: each fails to compile if the generated method
    // resolved anything other than the value type its declaration named.
    let _: Result<Option<u64>> = OwnStateKey.get_state();
    let _: Result<usize> = OwnStateKey.set_state(&1u64);
    let _: Result<()> = OwnStateKey.delete_state();
    let _: Result<Config> = OwnHookName.get_value();
    let _: Result<Config> = OwnOtxnName.get_value();
    let _: &'static [u8] = OwnHookName.get_name();

    // `Config` is a real multi-field value type, not an opaque marker —
    // its fields are what a live read would decode into.
    assert_eq!(Config { min_amount: 5 }.min_amount, 5);
}

/// `get_name` is `const fn`, so a fixed name's bytes are usable wherever a
/// constant is — no runtime encode, nothing to inline away.
const OWN_HOOK_NAME_BYTES: &[u8] = OwnHookName.get_name();

#[test]
fn get_name_is_usable_in_const_context() {
    assert_eq!(OWN_HOOK_NAME_BYTES, b"OHN".as_slice());
}

// ---------------------------------------------------------------------------
// Form 4 (newtype): the one declaring form a binder cannot take
// ---------------------------------------------------------------------------

// A newtype state key and a newtype parameter name, both at module scope
// (Form 4 rejects an instance binder — its instance needs the inner value,
// which the grammar has nowhere to spell — so this is how it is written).
hook_state!(AccountKey AccountId => u64);
hook_parameter!(TopicName [u8; 4] => Config);
otxn_parameter!(OtxnTopicName [u8; 4] => Config);

#[test]
fn newtype_form_carries_the_same_methods() {
    let key = AccountKey(AccountId::default());
    assert_eq!(key.get_state().err(), Some(STUB));
    assert_eq!(key.set_state(&1u64), Err(STUB));
    assert_eq!(key.update_state(|current| current.unwrap_or(0)), Err(STUB));
    assert_eq!(key.delete_state(), Err(STUB));

    let name = TopicName(*b"TOPC");
    assert_eq!(name.get_value().err(), Some(STUB));
    assert_eq!(OtxnTopicName(*b"TOPC").get_value().err(), Some(STUB));

    // Value types resolved from the declaration, not from these bindings.
    let _: Result<Option<u64>> = key.get_state();
    let _: Result<Config> = name.get_value();
    let _: Result<Config> = OtxnTopicName(*b"TOPC").get_value();
}

// ---------------------------------------------------------------------------
// `state_delete`
// ---------------------------------------------------------------------------

#[test]
fn state_delete_is_reachable_as_a_free_function_and_as_a_method() {
    hook_state!(marker, MarkerKey = b"MRK" => u64);

    // Routing only: the host stub cannot demonstrate that an empty write
    // deletes the entry (see this file's module doc comment).
    assert_eq!(state_delete(&MarkerKey), Err(STUB));
    assert_eq!(marker.delete_state(), Err(STUB));
}

#[test]
fn state_delete_accepts_any_state_key_encode_key() {
    // No `TypedStateKey` pairing needed: deletion has no value type.
    assert_eq!(state_delete(&[0u8; 4]), Err(STUB));
}

// ---------------------------------------------------------------------------
// Lint gate: unused binders, unused methods
// ---------------------------------------------------------------------------

#[test]
fn unused_binders_are_lint_clean() {
    // None of these three binders is ever read, and none of the methods
    // they bring along is ever called. Under `mise run lint`
    // (`--all-targets -D warnings`) this test compiles only because the
    // expansion carries `#[allow(unused_variables)]` on the `let` (plus
    // `unused_mut` for the struct form) and `#[allow(dead_code)]` on every
    // generated method.
    hook_state!(unused_state, UnusedStateKey = b"US" => u64);
    hook_parameter!(unused_hook_param, UnusedHookName = b"UH" => Config);
    otxn_parameter!(unused_otxn_param, UnusedOtxnName = b"UO" => Config);
    hook_state!(unused_struct, UnusedStructKey {tag: u8} = {tag: 0} => u64);

    // Form 4 takes no binder, so its accessors reach the lint gate only
    // through a declaration whose type is constructed but whose generated
    // methods are all left uncalled.
    hook_state!(UnusedNewtypeKey [u8; 4] => u64);
    hook_parameter!(UnusedNewtypeName [u8; 4] => Config);
    let _ = UnusedNewtypeKey(*b"UNTK");
    let _ = UnusedNewtypeName(*b"UNTN");
}
