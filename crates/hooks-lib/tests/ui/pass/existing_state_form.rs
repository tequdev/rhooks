//! The `existing` keyword form on the state side — a capability the
//! parameters-only comma-form never had: fixed key bytes and a value
//! pairing attached to a key type the caller declares (and documents, and
//! gives a visibility to) itself.

use hooks_lib::hook_state;
use hooks_lib::prelude::*;

/// A key the caller owns outright.
pub struct RewardRateKey;

hook_state!(existing RewardRateKey = b"RR" => u64);

fn main() {
    assert_eq!(
        RewardRateKey.get_state().err(),
        Some(HookError::NotImplemented)
    );
    assert_eq!(RewardRateKey.delete_state(), Err(HookError::NotImplemented));
}
