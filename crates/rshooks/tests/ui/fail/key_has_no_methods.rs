//! The accessors live on the **entity**, and only there. The key type is a
//! trait carrier: it encodes, and that is all. Pinned so the v1 behavior
//! (methods on the key) cannot come back by accident.

use rshooks::prelude::*;
use rshooks::hook_state;

hook_state!(RewardRate, RewardRateKey = b"RR" => u64);
hook_state!(DepositState, DepositKey {tag: u8} => DepositValue {amount: u64});

fn main() {
    // The key type has no inherent accessors...
    let _ = RewardRateKey.get_state();
    let _ = DepositKey { tag: 1 }.set_state(&DepositValue { amount: 1 });

    // ...though it still encodes, which is what it is for.
    let _ = state_get_typed(&RewardRateKey);
}
