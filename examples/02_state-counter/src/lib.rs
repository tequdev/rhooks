#![no_std]

use hooks_lib::{accept, hook, hook_errors, hook_state, rollback};

hook_state!(Counter, CounterKey {name: [u8; 7]} = {name: *b"counter"} => u64);

hook_errors! {
    /// Errors returned by the counter hook.
    pub enum StateCounterError {
        /// The updated counter could not be stored.
        StateSetFailed = 1,
    }
}

#[hook]
fn my_hook() -> i64 {
    let count = Counter.get_state().unwrap_or(None).unwrap_or(0);

    let next = count.wrapping_add(1);
    if Counter.set_state(&next).is_err() {
        rollback!(
            b"state-counter: state_set failed",
            StateCounterError::StateSetFailed
        );
    }

    accept!(b"state-counter: incremented", next as i64)
}
