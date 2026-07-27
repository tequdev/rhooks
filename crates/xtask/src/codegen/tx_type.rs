//! Generates `crates/hooks-lib/src/tx_type.rs`: a typed `TxType` enum
//! mirroring `hooks_core::tts`'s raw `tt*` transaction-type constants, from
//! `tts.h`'s parsed [`ConstSpec`]s (`crates/xtask/src/ir.rs`, `hook_api.json`)
//! — the same source data [`super::tts`] renders as hooks-core's raw `u16`
//! constants. Unlike every other generator in this module, the output
//! lands in `hooks-lib`, not `hooks-core`: `TxType` is a typed,
//! Rust-idiomatic mirror (the `hooks-lib` layer's job, per `docs/DESIGN.md`
//! §5), not a mechanical 1:1 header translation (`hooks-core`'s job, per
//! §4) — but it is still fully mechanical *within* that typed layer (every
//! variant name is a pure function of its `tt*` name, no hand-authored
//! per-variant text), so it is generated rather than hand-maintained, the
//! same way `tts.rs` itself is.

use std::fmt::Write as _;

use anyhow::{Context, Result, anyhow};

use super::with_generated_marker;
use crate::ir::ConstSpec;
use crate::render::expect_decimal;

const MODULE_DOC: &str = "\
//! Transaction type (`TxType`) model.
//!
//! [`TxType`] is a typed, exhaustive-by-construction mirror of the raw
//! `tt*` transaction-type codes in `hooks_core::tts` (plus
//! [`TxType::Unknown`] for forward-compatibility with codes this crate
//! does not yet know about) — the same pattern [`crate::error::HookError`]
//! uses for the Hook API's negative error-code channel, applied here to
//! [`crate::api::otxn::otxn_type`]'s `u16` transaction-type channel.
";

/// Converts a C `tt*` constant name (e.g. `ttNFTOKEN_MINT`) into a
/// PascalCase enum variant name (`NftokenMint`): strips the `tt` prefix,
/// splits the remainder on `_`, and capitalizes each segment's first
/// letter while lowercasing the rest. Deliberately does not special-case
/// acronyms (upstream's own preferred `NFToken`/`URIToken`/`XChain`
/// capitalization) — one unconditional mechanical rule, not a lookup
/// table of exceptions, matches `hooks-core`'s own "no renaming" principle
/// applied one layer up.
fn variant_name(const_name: &str) -> Result<String> {
    let rest = const_name
        .strip_prefix("tt")
        .ok_or_else(|| anyhow!("expected a `tt`-prefixed name, got `{const_name}`"))?;
    let mut out = String::new();
    for part in rest.split('_') {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            for c in chars {
                out.extend(c.to_lowercase());
            }
        }
    }
    Ok(out)
}

/// Renders `tx_type.rs`'s full contents from `tts.h`'s parsed
/// [`ConstSpec`]s.
pub fn generate(tts: &[ConstSpec]) -> Result<String> {
    let mut variants = String::new();
    let mut from_arms = String::new();
    let mut code_arms = String::new();
    let mut known_codes = Vec::with_capacity(tts.len());

    for d in tts {
        let value = expect_decimal(&d.name, &d.c_expr)?;
        let variant = variant_name(&d.name)?;
        writeln!(variants, "    /// `{}` ({value}).", d.name).context("writing variant doc")?;
        writeln!(variants, "    {variant},").context("writing variant")?;
        writeln!(
            from_arms,
            "            hooks_core::{} => TxType::{variant},",
            d.name
        )
        .context("writing From arm")?;
        writeln!(
            code_arms,
            "            TxType::{variant} => hooks_core::{},",
            d.name
        )
        .context("writing code() arm")?;
        known_codes.push(value);
    }
    let known_codes = known_codes.join(", ");

    let mut body = String::from("\n");
    body.push_str(
        "/// The transaction type of the originating transaction, decoded from the\n\
         /// raw `u16` `tt*` code returned by [`crate::api::otxn::otxn_type`].\n\
         ///\n\
         /// # Examples\n\
         ///\n\
         /// ```\n\
         /// use hooks_lib::tx_type::TxType;\n\
         ///\n\
         /// let ty = TxType::from(5);\n\
         /// assert_eq!(ty, TxType::RegularKeySet);\n\
         /// assert_eq!(ty.code(), 5);\n\
         /// ```\n\
         #[derive(Debug, Clone, Copy, PartialEq, Eq)]\n\
         pub enum TxType {\n",
    );
    body.push_str(&variants);
    body.push('\n');
    body.push_str(
        "    /// A code this version of hooks-lib does not recognize yet. Carries\n\
         /// the raw code for forward-compatibility.\n\
         Unknown(u16),\n\
         }\n\
         \n\
         impl From<u16> for TxType {\n\
         fn from(code: u16) -> Self {\n\
         match code {\n",
    );
    body.push_str(&from_arms);
    body.push_str(
        "            other => TxType::Unknown(other),\n\
         }\n\
         }\n\
         }\n\
         \n\
         impl TxType {\n\
         /// The raw `u16` code this variant corresponds to. Exact inverse of\n\
         /// [`TxType::from`]: `TxType::from(c).code() == c` for every code,\n\
         /// known or unknown.\n\
         #[inline(always)]\n\
         #[must_use]\n\
         pub fn code(&self) -> u16 {\n\
         match *self {\n",
    );
    body.push_str(&code_arms);
    body.push_str(&format!(
        "            TxType::Unknown(code) => code,\n\
         }}\n\
         }}\n\
         }}\n\
         \n\
         #[cfg(test)]\n\
         mod tests {{\n\
         use super::*;\n\
         \n\
         #[test]\n\
         fn round_trips_known_codes() {{\n\
         let known: &[u16] = &[{known_codes}];\n\
         for &code in known {{\n\
         assert_eq!(\n\
         TxType::from(code).code(),\n\
         code,\n\
         \"round-trip failed for {{code}}\"\n\
         );\n\
         }}\n\
         }}\n\
         \n\
         #[test]\n\
         fn unknown_code_round_trips() {{\n\
         let ty = TxType::from(9999);\n\
         assert_eq!(ty, TxType::Unknown(9999));\n\
         assert_eq!(ty.code(), 9999);\n\
         }}\n\
         }}\n"
    ));

    Ok(with_generated_marker("tts.h", MODULE_DOC) + &body)
}
