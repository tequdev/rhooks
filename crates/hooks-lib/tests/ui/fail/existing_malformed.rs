//! Malformed tails after the `existing` keyword.
//!
//! This form accepts exactly one shape, so every way of getting it wrong
//! gets the same targeted diagnostic naming that shape — never the binder
//! diagnostic, and never the token-run collector's generic "expected a type
//! before `=>`" (which describes the wrong thing entirely once the caller
//! has written `existing`).

use hooks_lib::{ParamValue, hook_parameter};

#[derive(ParamValue)]
struct Config {
    min_amount: u64,
}

struct CfgName;
struct InsName;

// No `= bytes` at all: the two-type pairing shape, which `existing` does not
// accept.
hook_parameter!(existing CfgName => Config);

// The `=` is there, but the name bytes are not.
hook_parameter!(existing InsName = => Config);

fn main() {}
