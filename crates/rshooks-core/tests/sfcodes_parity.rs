//! Parity test: `vendor/xahaud-hook/sfcodes.h` `#define`s vs
//! `src/sfcodes.rs` `pub const`s (`docs/DESIGN.md` §4). Values are
//! shift-add expressions like `((16U << 16U) + 1U)`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::{assert_maps_match, build_env, extract_c_defines, extract_rust_consts};

const HEADER: &str = include_str!("../vendor/xahaud-hook/sfcodes.h");
const RUST: &str = include_str!("../src/sfcodes.rs");

#[test]
fn sfcodes_h_matches_sfcodes_rs() {
    let header_defs = extract_c_defines(HEADER);
    assert_eq!(
        header_defs.len(),
        325,
        "expected 325 #define entries in sfcodes.h, found {}",
        header_defs.len()
    );
    let header_env = build_env(&header_defs);

    let rust_consts = extract_rust_consts(RUST);
    assert_eq!(
        rust_consts.len(),
        325,
        "expected 325 pub const entries in sfcodes.rs, found {}",
        rust_consts.len()
    );
    for (name, ty, _) in &rust_consts {
        assert_eq!(ty, "u32", "{name}: expected type u32, found {ty}");
    }
    let rust_defs: Vec<(String, String)> =
        rust_consts.into_iter().map(|(n, _, e)| (n, e)).collect();
    let rust_env = build_env(&rust_defs);

    assert_maps_match("sfcodes.h", &header_env, "sfcodes.rs", &rust_env);
}
