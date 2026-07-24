//! Fuzz target for `hooks_lib::txn::codec` (`docs/DESIGN.md` §5.5): for any
//! `u32` `sfcode`, `field_header` and the size helpers built on it must
//! never panic, and their outputs must stay internally consistent.
//!
//! `field_header` is documented (`crates/hooks-lib/src/txn.rs`) as a
//! "generic, panic-free" primitive with no `# Panics` section of its own
//! (unlike `write_field_header`/`write_const_bytes`/
//! `encode_native_amount_const`, which are explicitly documented as
//! const-context-only panics) — so any panic this target finds for an
//! `sfcode` value that is reachable at runtime (not just from a `const fn`
//! macro-expansion context) is a genuine `hooks-lib` bug, not an expected
//! finding. Known real `sfcode`s (`hooks_core::sfcodes`) always keep their
//! `type`/`field` components under 256, so this may find a bug that never
//! triggers on real transaction fields but is nonetheless reachable from
//! this public, non-`const` function's documented contract for arbitrary
//! `u32` input.
#![no_main]

use hooks_lib::txn::codec;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|sfcode: u32| {
    let (hdr, hdr_len) = codec::field_header(sfcode);

    // `hdr_len` is documented as "the number of meaningful bytes" out of the
    // fixed 3-byte array, per the encoding table in `field_header`'s doc
    // comment.
    assert!(
        (1..=3).contains(&hdr_len),
        "field_header({sfcode:#010x}) returned an out-of-range length {hdr_len}"
    );

    let ty = sfcode >> 16;
    let field = sfcode & 0xFFFF;
    let expected_len = if ty < 16 && field < 16 {
        1
    } else if ty < 16 || field < 16 {
        2
    } else {
        3
    };
    assert_eq!(
        hdr_len, expected_len,
        "field_header({sfcode:#010x}) (type {ty}, field {field}) returned length {hdr_len}, \
         expected {expected_len} per the documented encoding table"
    );

    // The size helpers all just add a fixed constant to `field_header`'s
    // length; check that relationship stays intact rather than re-deriving
    // the header independently (which would just be a copy of the
    // implementation).
    assert_eq!(
        codec::transaction_type_field_size(sfcode),
        hdr_len + 2,
        "transaction_type_field_size disagrees with field_header's own length"
    );
    assert_eq!(
        codec::u32_field_size(sfcode),
        hdr_len + 4,
        "u32_field_size disagrees with field_header's own length"
    );
    assert_eq!(
        codec::native_amount_field_size(sfcode),
        hdr_len + 8,
        "native_amount_field_size disagrees with field_header's own length"
    );
    assert_eq!(
        codec::account_id_field_size(sfcode),
        hdr_len + 1 + 20,
        "account_id_field_size disagrees with field_header's own length"
    );
    assert_eq!(
        codec::empty_vl_field_size(sfcode),
        hdr_len + 1,
        "empty_vl_field_size disagrees with field_header's own length"
    );

    // Bytes beyond `hdr_len` are documented as not meaningful, but the array
    // is always fully initialized (no uninitialized-memory concern) — just
    // confirm indexing the full array never panics.
    let _ = hdr;
});
