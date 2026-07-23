//! Parity test: `vendor/xahaud-hook/ls_flags.h` enum members vs
//! `src/ls_flags.rs` `pub const`s (`docs/DESIGN.md` §4). The header groups
//! these into several unnamed-in-Rust C enums; the translation flattens
//! them into one list, in header order.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::{assert_maps_match, build_env, extract_c_enum_members, extract_rust_consts};

const HEADER: &str = include_str!("../vendor/xahaud-hook/ls_flags.h");
const RUST: &str = include_str!("../src/ls_flags.rs");

#[test]
fn ls_flags_h_matches_ls_flags_rs() {
    let header_members = extract_c_enum_members(HEADER);
    assert_eq!(
        header_members.len(),
        46,
        "expected 46 enum members in ls_flags.h, found {}",
        header_members.len()
    );
    let header_env = build_env(&header_members);

    let rust_consts = extract_rust_consts(RUST);
    assert_eq!(
        rust_consts.len(),
        46,
        "expected 46 pub const entries in ls_flags.rs, found {}",
        rust_consts.len()
    );
    for (name, ty, _) in &rust_consts {
        assert_eq!(ty, "u32", "{name}: expected type u32, found {ty}");
    }
    let rust_defs: Vec<(String, String)> =
        rust_consts.into_iter().map(|(n, _, e)| (n, e)).collect();
    let rust_env = build_env(&rust_defs);

    assert_maps_match("ls_flags.h", &header_env, "ls_flags.rs", &rust_env);
}
