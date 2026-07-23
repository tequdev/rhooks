//! Drift tripwire for the vendored Hook API headers (`docs/DESIGN.md` §4),
//! identical in spirit to
//! `crates/hooks-build/tests/guard_native.rs::vendored_files_match_recorded_sha256`.
//!
//! Test code is exempt from the workspace's panic-freedom lints (per
//! `docs/DESIGN.md` §8): `unwrap`/`expect` on a known-good fixture is the
//! normal, idiomatic way to assert behavior in a test.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use sha2::{Digest, Sha256};

/// Drift tripwire against `vendor/xahaud-hook/SHA256SUMS` (the single source
/// of truth for the vendored hashes, regenerated only by
/// `scripts/sync-vendor.sh`): an accidental local edit to the vendored,
/// supposedly byte-identical upstream headers (or a corrupted re-download)
/// fails a test loudly, instead of silently diverging from what a real
/// xahaud node runs — and, transitively, from what the parity tests in this
/// same directory assume they're checking.
#[test]
fn vendored_files_match_recorded_sha256() {
    fn sha256_hex(path: &str) -> String {
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    let sums = std::fs::read_to_string("vendor/xahaud-hook/SHA256SUMS")
        .expect("reading vendor/xahaud-hook/SHA256SUMS");
    let mut checked = 0;
    for line in sums.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (want, name) = line
            .split_once("  ")
            .unwrap_or_else(|| panic!("malformed SHA256SUMS line: {line:?}"));
        let path = format!("vendor/xahaud-hook/{name}");
        let got = sha256_hex(&path);
        assert_eq!(
            got, want,
            "{path} sha256 mismatch — the vendored file has drifted from \
             vendor/xahaud-hook/SHA256SUMS; never hand-edit vendored files, \
             re-sync with scripts/sync-vendor.sh (see VENDOR.md)"
        );
        checked += 1;
    }
    assert_eq!(
        checked, 8,
        "expected exactly 8 entries in vendor/xahaud-hook/SHA256SUMS"
    );
}
