//! The optional leading visibility, applied to every declared item and
//! exercised from a **sibling module** — the only place a wrong visibility
//! would actually show up.
//!
//! Every shape is constructed and used across the module boundary: the
//! entity and its fields, the key struct and its fields, the inline value
//! struct and its fields, a Form 2 `const`, a pairing entity's `pub E(K)`
//! tuple field, and a public entity sitting over a **private** `existing`
//! key — the one caller-owned type the visibility precondition exempts,
//! because it never reaches the entity's public API.

use rshooks::prelude::*;

mod ledger {
    use rshooks::prelude::*;
    use rshooks::{HookData, HookKey, hook_parameter, hook_state};

    // Form 3: entity + key + inline value, all public, fields included.
    hook_state!(pub DepositState, DepositKey {tag: u8, owner: AccountId} => Deposit {amount: u64, deadline: u32});

    // Form 2: the `const` is public too.
    hook_state!(pub Counter, CounterKey {name: [u8; 7]} = {name: *b"counter"} => u64);

    // Form 1 on the parameter side.
    hook_parameter!(pub Cfg, CfgName = b"CFG" => Config {min_amount: u64});

    // Form 4, whose `Inner` is a type from another crate — already at least
    // as visible as this invocation, which is the precondition.
    hook_state!(pub AccountState, AccountKey AccountId => u64);

    // `pub(crate)` restricts to the crate, and applies just as uniformly.
    hook_state!(pub(crate) Marker, MarkerKey = b"MK" => u64);

    /// A key this module owns, paired below. `pub` because it becomes the
    /// public entity's tuple field — the precondition in action.
    #[derive(HookKey, Clone, Copy)]
    pub struct SeatKey {
        /// Which seat.
        pub seat: u8,
    }

    /// The value that key holds.
    #[derive(HookData, Clone, Copy)]
    pub struct SeatValue {
        /// Vote count.
        pub votes: u32,
    }

    // Pairing: the entity's tuple field carries the invocation's visibility,
    // so `SeatState(SeatKey { .. })` is constructible from outside.
    hook_state!(pub SeatState, SeatKey => SeatValue);

    /// A key type this module keeps **private**, attached with `existing`
    /// under a `pub` invocation. Legal precisely because an `existing` key
    /// never appears in the entity's public fields or associated types.
    struct PrivateKey;

    hook_state!(pub PrivateBacked, existing PrivateKey = b"PB" => u64);
}

fn main() {
    // Entity: constructed from outside, its fields written and read.
    let mut deposit = ledger::DepositState {
        tag: 1,
        owner: AccountId::default(),
    };
    deposit.tag = 2;
    assert_eq!(deposit.tag, 2);
    assert_eq!(deposit.get_state().err(), Some(HookError::NotImplemented));

    // The public inline value, constructed and read from outside.
    let value = ledger::Deposit {
        amount: 1,
        deadline: 2,
    };
    assert_eq!(value.amount, 1);
    assert_eq!(deposit.set_state(&value), Err(HookError::NotImplemented));

    // The public key component, usable with the free functions.
    let key = ledger::DepositKey {
        tag: 1,
        owner: AccountId::default(),
    };
    assert_eq!(state_get_typed(&key).err(), Some(HookError::NotImplemented));

    // The Form 2 `const`, and its field.
    assert_eq!(ledger::Counter.name, *b"counter");
    assert_eq!(
        ledger::Counter.get_state().err(),
        Some(HookError::NotImplemented)
    );

    // Parameter side.
    assert_eq!(ledger::Cfg.get_name(), b"CFG".as_slice());
    assert_eq!(ledger::Cfg.get_value().err(), Some(HookError::NotImplemented));

    // Form 4, constructed across the boundary.
    let account = ledger::AccountState(AccountId::default());
    assert_eq!(account.get_state().err(), Some(HookError::NotImplemented));

    // `pub(crate)` reaches here (same crate).
    assert_eq!(
        ledger::Marker.get_state().err(),
        Some(HookError::NotImplemented)
    );

    // A pairing entity, constructed across the boundary through its public
    // tuple field.
    let seat = ledger::SeatState(ledger::SeatKey { seat: 1 });
    assert_eq!(seat.get_state().err(), Some(HookError::NotImplemented));
    assert_eq!(
        seat.set_state(&ledger::SeatValue { votes: 2 }),
        Err(HookError::NotImplemented)
    );

    // A public entity over a private `existing` key: usable from here even
    // though `ledger::PrivateKey` is not nameable from here at all.
    assert_eq!(
        ledger::PrivateBacked.get_state().err(),
        Some(HookError::NotImplemented)
    );
}
