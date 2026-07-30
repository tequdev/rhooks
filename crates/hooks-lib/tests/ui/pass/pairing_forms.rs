//! The pairing form over the three shapes of caller-owned key it has to
//! support, including the two the forwarding design exists for: a key with
//! `StateKeyEncode` but no `ToBytes`, and a key that is not `Copy`.
//!
//! Nothing generated constructs or copies the key — the entity borrows the
//! one the caller put in it — which is what makes both cases work.

use hooks_lib::prelude::*;
use hooks_lib::{HookData, HookKey, ParamName, ParamValue, hook_parameter, hook_state, state_keys};

#[derive(HookKey, Clone, Copy)]
struct PairedKey {
    tag: u8,
}

#[derive(HookData, Clone, Copy)]
struct PairedValue {
    count: u32,
}

hook_state!(PairedState, PairedKey => PairedValue);

// A `state_keys!` enum has `StateKeyEncode` but no `ToBytes`: the entity
// forwards `encode` rather than re-encoding fields, so this pairs fine.
state_keys! {
    /// This hook's persistent data.
    enum DataKey {
        /// A running counter.
        Counter,
        /// A per-owner balance.
        Balance(AccountId),
    }
}

hook_state!(DataState, DataKey => u64);

// Deliberately not `Copy`/`Clone`.
#[derive(HookKey)]
struct NonCopyKey {
    tag: u8,
    owner: AccountId,
}

hook_state!(NonCopyState, NonCopyKey => u64);

// Parameter side: `with_name_bytes` forwards to the name's own override,
// keeping its exact-size buffer.
#[derive(ParamName, Clone, Copy)]
struct SeatName {
    topic: u8,
    seat: u8,
}

#[derive(ParamValue)]
struct Vote {
    value: u8,
}

hook_parameter!(SeatVote, SeatName => Vote);

fn main() {
    assert_eq!(
        PairedState(PairedKey { tag: 0 }).get_state().err(),
        Some(HookError::NotImplemented)
    );
    assert_eq!(
        DataState(DataKey::Counter).get_state().err(),
        Some(HookError::NotImplemented)
    );
    assert_eq!(
        DataState(DataKey::Balance(AccountId::default()))
            .get_state()
            .err(),
        Some(HookError::NotImplemented)
    );

    let non_copy = NonCopyState(NonCopyKey {
        tag: 1,
        owner: AccountId::default(),
    });
    assert_eq!(non_copy.get_state().err(), Some(HookError::NotImplemented));
    assert_eq!(non_copy.delete_state(), Err(HookError::NotImplemented));

    let seat = SeatVote(SeatName {
        topic: b'S',
        seat: 0,
    });
    assert_eq!(seat.get_value().err(), Some(HookError::NotImplemented));
    assert_eq!(Vote { value: 1 }.value, 1);
}
