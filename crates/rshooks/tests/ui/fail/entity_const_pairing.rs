//! `Entity, SOME_CONST => V` is indistinguishable from the pairing form: a
//! bare identifier after the entity's comma is a key *type*, and the macro
//! has no way to know the caller meant a constant. It parses as a pairing
//! and rustc reports "expected type" against the generated impls.
//!
//! Pinned deliberately as a **documented diagnostic regression** — the
//! alternative (rejecting SCREAMING_SNAKE_CASE key types) would reject
//! legitimate type names for the sake of a mistake no caller has made.

use rshooks::{ParamValue, hook_parameter};

const CFG_NAME: &[u8] = b"CFG";

#[derive(ParamValue)]
struct Config {
    min_amount: u64,
}

hook_parameter!(Cfg, CFG_NAME => Config);

fn main() {}
