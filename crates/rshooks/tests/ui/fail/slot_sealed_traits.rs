//! `SlotKey` and `CastTarget` are sealed. A downstream `SlotKey` could hand
//! back an arbitrary slot number and forge a handle aliasing a slot something
//! else owns; a downstream `CastTarget` could accept every type ID. Neither
//! is expressible outside this crate.

use rshooks::prelude::*;
use rshooks::types::STObject;

struct MyKey;

impl SlotKey<STObject> for MyKey {
    type Out = STObject;
}

struct MyTarget;

impl CastTarget for MyTarget {}

fn main() {}
