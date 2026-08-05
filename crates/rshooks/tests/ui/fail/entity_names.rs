//! Every way the mandatory leading entity name can be wrong.
//!
//! One fixture per rule, not per case: each invocation fails independently
//! during expansion, so rustc reports all of them in a single compilation.

use rshooks::{ParamValue, hook_parameter, hook_state};

/// A key type the module owns, for the `existing` case below.
struct OwnKey;

/// A value for the parameter-macro case below.
#[derive(ParamValue)]
struct Config {
    min_amount: u64,
}

fn main() {
    // No entity at all — the declaration starts where the entity should be.
    hook_state!(RewardRateKey = b"RR" => u64);

    // A snake_case leading identifier: the shape of the removed instance
    // binder, so the diagnostic carries both migration recipes.
    hook_state!(reward_rate, RewardRateKey = b"RR" => u64);

    // `existing` in entity position — the entity name was forgotten.
    hook_state!(existing, OwnKey = b"OK" => u64);

    // An entity name that is not UpperCamelCase.
    hook_state!(Reward_Rate, RewardRateKey = b"RR" => u64);

    // A literal where the declaration should start.
    hook_state!(RewardRate, b"RR" => u64);

    // `Self` is UpperCamelCase but cannot name a declared type.
    hook_state!(Self, RewardRateKey = b"RR" => u64);
    hook_state!(RewardRate, Self = b"RR" => u64);

    // The parameter macros illustrate `get_value()`, not `get_state()` —
    // their entities are read-only.
    hook_parameter!(CfgName = b"CFG" => Config);
}
