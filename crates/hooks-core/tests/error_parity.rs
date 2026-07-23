//! Parity test: `vendor/xahaud-hook/error.h` `#define`s vs `src/error.rs`
//! `pub const`s (`docs/DESIGN.md` §4).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
use common::{assert_maps_match, build_env, extract_c_defines, extract_rust_consts};

const HEADER: &str = include_str!("../vendor/xahaud-hook/error.h");
const RUST: &str = include_str!("../src/error.rs");

#[test]
fn error_h_matches_error_rs() {
    let header_defs = extract_c_defines(HEADER);
    assert_eq!(
        header_defs.len(),
        46,
        "expected 46 #define entries in error.h, found {}",
        header_defs.len()
    );
    let header_env = build_env(&header_defs);

    let rust_consts = extract_rust_consts(RUST);
    assert_eq!(
        rust_consts.len(),
        46,
        "expected 46 pub const entries in error.rs, found {}",
        rust_consts.len()
    );
    for (name, ty, _) in &rust_consts {
        assert_eq!(ty, "i64", "{name}: expected type i64, found {ty}");
    }
    let rust_defs: Vec<(String, String)> =
        rust_consts.into_iter().map(|(n, _, e)| (n, e)).collect();
    let rust_env = build_env(&rust_defs);

    assert_maps_match("error.h", &header_env, "error.rs", &rust_env);

    // The known irregular value, called out explicitly in both the header
    // and the Rust translation's doc comment.
    assert_eq!(header_env["INVALID_FLOAT"], -10024);
}
