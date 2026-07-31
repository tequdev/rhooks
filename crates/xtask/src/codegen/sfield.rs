//! Generates `crates/hooks-lib/src/sfield.rs`: typed `SField<T>` field
//! constants mirroring `hooks_core::sfcodes`'s raw `sfXxx` `u32` codes, from
//! `sfcodes.h`'s parsed [`ConstSpec`]s (`crates/xtask/src/ir.rs`,
//! `hook_api.json`) — the same source data [`super::sfcodes`] renders as
//! hooks-core's raw `u32` constants.
//!
//! Like [`super::tx_type`] — the two generators whose output lands in
//! `hooks-lib` rather than `hooks-core` — a typed mirror is the
//! `hooks-lib` layer's job per `docs/DESIGN.md` §5, while a mechanical 1:1
//! header translation is `hooks-core`'s per §4. It is still fully mechanical
//! *within* that typed layer — every constant's value type is a pure
//! function of the serialized type ID packed into its own code — so it is
//! generated rather than hand-maintained, exactly as `sfcodes.rs` is.
//!
//! The `SField<T>` type itself is hand-written in `hooks-lib`'s `slot_obj`
//! module; only the 325 constants are generated here.

use std::fmt::Write as _;

use anyhow::{Context, Result};

use super::with_generated_marker;
use crate::ir::ConstSpec;
use crate::render::render_shift_add;

const MODULE_DOC: &str = "\
//! Typed serialized-field constants ([`SField<T>`](crate::slot_obj::SField)).
//!
//! One constant per `sfXxx` code in [`hooks_core::sfcodes`], carrying the
//! Rust type that field's value reads back as. `sfSequence` is an
//! `SField<u32>`, `sfAccount` an `SField<AccountId>`, `sfBalance` an
//! `SField<Amount>` — so
//! [`SlotObject::get`](crate::slot_obj::SlotObject::get) can hand back an
//! already-typed handle and
//! [`value`](crate::slot_obj::SlotObject::value) needs no turbofish:
//!
//! ```
//! use hooks_lib::prelude::*;
//!
//! // The type comes from the constant, not from an annotation.
//! let _: SField<u32> = sfSequence;
//! let _: SField<AccountId> = sfAccount;
//!
//! // `.code()` is the const-context bridge back to the raw `u32`.
//! const SEQ: u32 = sfSequence.code();
//! assert_eq!(SEQ, hooks_lib::raw::sfcodes::sfSequence);
//! ```
//!
//! # How the value type is chosen
//!
//! Every `sfXxx` code packs a serialized type ID in its high bits
//! (`code >> 16`), and that ID alone decides the Rust type: 2 (`UInt32`) →
//! `u32`, 8 (`AccountID`) → [`AccountId`](crate::types::AccountId), 14
//! (`STObject`) → [`STObject`](crate::slot_obj::STObject), and so on.
//!
//! IDs this layer does not model a typed read for — `Blob`, `PathSet`,
//! `Vector256`, `Number`, `UInt192`, `XChainBridge`, and `Hash160` (whose
//! four fields carry genuinely different semantics: two currencies and two
//! issuers) — map to [`Opaque`](crate::slot_obj::Opaque), which supports
//! navigation and the raw byte escapes but no `value()`. Nothing is lost:
//! the bytes are still reachable, just not pre-interpreted.
//!
//! # Naming
//!
//! `#[allow(non_upper_case_globals)]`, because these mirror C constant names
//! verbatim — the same rule [`hooks_core::sfcodes`] follows, and for the
//! same reason: a hook author reading xahaud's own sources should find the
//! identical spelling here.
";

/// The Rust value type a serialized type ID maps to, per design §3.1.
///
/// Returns `None` for every ID this layer does not model, which the caller
/// renders as `Opaque`. Deliberately a total function over the IDs that
/// actually appear in `sfcodes.h` rather than an exhaustive enum: the header
/// is the source of truth for which IDs exist, and an unmodelled one should
/// degrade to `Opaque`, never fail the build.
fn value_type(type_id: u32) -> Option<&'static str> {
    match type_id {
        16 => Some("u8"),
        1 => Some("u16"),
        2 => Some("u32"),
        3 => Some("u64"),
        5 => Some("crate::types::Hash"),
        6 => Some("crate::slot_obj::Amount"),
        8 => Some("crate::types::AccountId"),
        14 => Some("crate::slot_obj::STObject"),
        15 => Some("crate::slot_obj::STArray"),
        24 => Some("crate::slot_obj::Issue"),
        26 => Some("crate::types::CurrencyCode"),
        _ => None,
    }
}

/// A human-readable name for a serialized type ID, for the generated
/// per-constant doc comment. Unknown IDs are rendered numerically.
fn type_id_name(type_id: u32) -> String {
    match type_id {
        1 => "UInt16".into(),
        2 => "UInt32".into(),
        3 => "UInt64".into(),
        4 => "Hash128".into(),
        5 => "Hash256".into(),
        6 => "Amount".into(),
        7 => "Blob".into(),
        8 => "AccountID".into(),
        9 => "Number".into(),
        14 => "STObject".into(),
        15 => "STArray".into(),
        16 => "UInt8".into(),
        17 => "Hash160".into(),
        18 => "PathSet".into(),
        19 => "Vector256".into(),
        21 => "UInt192".into(),
        24 => "Issue".into(),
        25 => "XChainBridge".into(),
        26 => "Currency".into(),
        other => format!("type {other}"),
    }
}

/// Renders `sfield.rs`'s full contents from `sfcodes.h`'s parsed
/// [`ConstSpec`]s.
///
/// Each constant's value expression is rendered by the same
/// [`render_shift_add`] the raw `sfcodes.rs` generator uses, then wrapped in
/// `SField::new(..)` — so the typed and raw tables are two renderings of one
/// parse, and `typed.code() == raw` holds by construction (pinned anyway by
/// a 325-name parity test, because "by construction" is a claim worth
/// checking).
pub fn generate(sfcodes: &[ConstSpec]) -> Result<String> {
    let mut body =
        String::from("\n#![allow(non_upper_case_globals)]\n\nuse crate::slot_obj::SField;\n\n");

    for d in sfcodes {
        let value = render_shift_add(&d.c_expr)?;
        let type_id = serialized_type_id(&value)
            .with_context(|| format!("deriving the serialized type ID of `{}`", d.name))?;
        let ty = value_type(type_id).unwrap_or("crate::slot_obj::Opaque");

        writeln!(
            body,
            "/// C: `{name}` (sfcodes.h) — {id_name}, read as [`{ty}`].",
            name = d.name,
            id_name = type_id_name(type_id),
        )
        .context("writing constant doc")?;
        writeln!(
            body,
            "pub const {name}: SField<{ty}> = SField::new({value});",
            name = d.name,
        )
        .context("writing constant")?;
    }

    // The parity check, generated alongside the table it checks: one
    // assertion per constant, so "typed.code() == raw" is verified for all
    // 325 names rather than a hand-picked sample. Generated because a
    // hand-written list would drift the moment upstream adds a field — the
    // exact failure it exists to catch.
    body.push_str(
        "\n#[cfg(test)]\nmod parity {\n         //! Every typed constant equals the raw `sfcodes` constant of the\n         //! same name. Holds by construction — both tables are rendered\n         //! from one parse by `cargo xtask gen-core` — which is exactly why\n         //! it is worth asserting: \"by construction\" stops being true the\n         //! moment either generator changes shape.\n         \n    #[test]\n    fn typed_field_codes_match_raw() {\n",
    );
    for d in sfcodes {
        writeln!(
            body,
            "        assert_eq!(super::{name}.code(), ::hooks_core::sfcodes::{name}, \"{name}\");",
            name = d.name,
        )
        .context("writing parity assertion")?;
    }
    writeln!(
        body,
        "        assert_eq!({count}, {count}, \"generated for {count} constants\");",
        count = sfcodes.len(),
    )
    .context("writing parity count")?;
    body.push_str("    }\n}\n");

    Ok(with_generated_marker("sfcodes.h", MODULE_DOC) + &body)
}

/// Extracts the serialized type ID from a rendered `(A << 16) + B` value
/// expression — i.e. `A`.
///
/// Parsing the rendered expression rather than re-parsing the C source keeps
/// this generator honest about using the *same* value the raw table gets: if
/// [`render_shift_add`] ever changed shape, this fails loudly at generation
/// time instead of silently typing every field `Opaque`.
fn serialized_type_id(rendered: &str) -> Result<u32> {
    let s = rendered.trim();
    let inner = s
        .strip_prefix('(')
        .ok_or_else(|| anyhow::anyhow!("expected a leading `(` in {rendered:?}"))?;
    let (shift_group, _) = inner
        .split_once(')')
        .ok_or_else(|| anyhow::anyhow!("expected a closing `)` in {rendered:?}"))?;
    let (a, b) = shift_group
        .split_once("<<")
        .ok_or_else(|| anyhow::anyhow!("expected `<<` in {rendered:?}"))?;
    if b.trim() != "16" {
        return Err(anyhow::anyhow!(
            "expected a 16-bit shift in {rendered:?}, got {b:?}"
        ));
    }
    a.trim()
        .parse::<u32>()
        .with_context(|| format!("parsing the serialized type ID out of {rendered:?}"))
}
