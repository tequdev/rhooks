//! Parity test: `vendor/xahaud-hook/tts.h` `#define`s vs `src/tts.rs`
//! `pub const`s (`docs/DESIGN.md` §4).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::{assert_maps_match, build_env, extract_c_defines, extract_rust_consts};

const HEADER: &str = include_str!("../vendor/xahaud-hook/tts.h");
const RUST: &str = include_str!("../src/tts.rs");

#[test]
fn tts_h_matches_tts_rs() {
    let header_defs = extract_c_defines(HEADER);
    assert_eq!(
        header_defs.len(),
        74,
        "expected 74 #define entries in tts.h, found {}",
        header_defs.len()
    );
    let header_env = build_env(&header_defs);

    let rust_consts = extract_rust_consts(RUST);
    assert_eq!(
        rust_consts.len(),
        74,
        "expected 74 pub const entries in tts.rs, found {}",
        rust_consts.len()
    );
    for (name, ty, _) in &rust_consts {
        assert_eq!(ty, "u16", "{name}: expected type u16, found {ty}");
    }
    let rust_defs: Vec<(String, String)> =
        rust_consts.into_iter().map(|(n, _, e)| (n, e)).collect();
    let rust_env = build_env(&rust_defs);

    assert_maps_match("tts.h", &header_env, "tts.rs", &rust_env);
}
