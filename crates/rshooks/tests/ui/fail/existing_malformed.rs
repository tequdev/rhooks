//! Malformed tails after the `existing` keyword.
//!
//! This form accepts exactly one shape, so every way of getting it wrong
//! gets the same targeted diagnostic naming that shape — never the binder
//! diagnostic, and never the token-run collector's generic "expected a type
//! before `=>`" (which describes the wrong thing entirely once the caller
//! has written `existing`).

use rshooks::{ParamValue, hook_parameter};

#[derive(ParamValue)]
struct Config {
    min_amount: u64,
}

struct CfgName;
struct InsName;

// No `= bytes` at all: the pairing shape, which `existing` does not accept.
// The path grammar stops at the `=>` instead of swallowing `=> Config` into
// the type name.
hook_parameter!(Cfg, existing CfgName => Config);

// The `=` is there, but the name bytes are not.
hook_parameter!(Ins, existing InsName = => Config);

// A literal where the caller's type name belongs.
hook_parameter!(Lit, existing b"CFG" = b"CFG" => Config);

// A path that stops mid-way.
hook_parameter!(Partial, existing owned:: = b"CFG" => Config);

fn main() {}
