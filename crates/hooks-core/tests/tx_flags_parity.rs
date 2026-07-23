//! Parity test: `vendor/xahaud-hook/tx_flags.h` enum members vs
//! `src/tx_flags.rs` `pub const`s (`docs/DESIGN.md` §4).
//!
//! A handful of `tx_flags.h` members (the `MPTokenIssuanceCreateFlags` enum)
//! are defined as aliases of `ls_flags.h` values (`tfMPTCanLock =
//! lsfMPTCanLock`), and `tx_flags.rs` mirrors that with references into
//! `ls_flags` (`ls_flags::lsfMPTCanLock`) rather than re-typed literals — so
//! both sides are evaluated against an environment seeded from `ls_flags`
//! first, then filtered back down to just the `tx_flags` names for
//! comparison.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};

mod common;
use common::{assert_maps_match, build_env, extract_c_enum_members, extract_rust_consts};

const LS_HEADER: &str = include_str!("../vendor/xahaud-hook/ls_flags.h");
const TX_HEADER: &str = include_str!("../vendor/xahaud-hook/tx_flags.h");
const LS_RUST: &str = include_str!("../src/ls_flags.rs");
const TX_RUST: &str = include_str!("../src/tx_flags.rs");

#[test]
fn tx_flags_h_matches_tx_flags_rs() {
    let ls_members = extract_c_enum_members(LS_HEADER);
    let tx_members = extract_c_enum_members(TX_HEADER);
    assert_eq!(
        tx_members.len(),
        63,
        "expected 63 enum members in tx_flags.h, found {}",
        tx_members.len()
    );
    let tx_names: BTreeSet<String> = tx_members.iter().map(|(n, _)| n.clone()).collect();

    let mut combined = ls_members;
    combined.extend(tx_members);
    let combined_env = build_env(&combined);
    let header_env: BTreeMap<String, i64> = combined_env
        .into_iter()
        .filter(|(k, _)| tx_names.contains(k))
        .collect();

    let ls_consts = extract_rust_consts(LS_RUST);
    let tx_consts = extract_rust_consts(TX_RUST);
    assert_eq!(
        tx_consts.len(),
        63,
        "expected 63 pub const entries in tx_flags.rs, found {}",
        tx_consts.len()
    );
    for (name, ty, _) in &tx_consts {
        assert_eq!(ty, "u32", "{name}: expected type u32, found {ty}");
    }

    let mut combined_rust: Vec<(String, String)> =
        ls_consts.into_iter().map(|(n, _, e)| (n, e)).collect();
    combined_rust.extend(tx_consts.into_iter().map(|(n, _, e)| (n, e)));
    let combined_rust_env = build_env(&combined_rust);
    let rust_env: BTreeMap<String, i64> = combined_rust_env
        .into_iter()
        .filter(|(k, _)| tx_names.contains(k))
        .collect();

    assert_maps_match("tx_flags.h", &header_env, "tx_flags.rs", &rust_env);
}
