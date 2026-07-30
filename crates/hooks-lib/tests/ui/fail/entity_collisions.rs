//! The entity, the key/name and the value are three separate items, so
//! spelling any two of them the same way is a duplicate definition. Caught
//! at the caller's own tokens rather than left to rustc's report from
//! inside the expansion.

use hooks_lib::hook_state;

fn main() {
    // Entity == key.
    hook_state!(Deposit, Deposit {tag: u8} => DepositValue {amount: u64});

    // Entity == value (inline).
    hook_state!(Balance, BalanceKey {tag: u8} => Balance {amount: u64});

    // Key == value (both declared).
    hook_state!(RewardState, Reward {tag: u8} => Reward {amount: u64});

    // Entity == value, where the value is an already-declared type.
    hook_state!(Amount, AmountKey {tag: u8} => Amount);
}
