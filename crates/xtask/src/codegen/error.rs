//! Generates `crates/hooks-core/src/error.rs` from `error.h`'s parsed
//! [`ConstSpec`]s (`crates/xtask/src/ir.rs`, `hook_api.json`).

use anyhow::Result;

use super::{push_const, with_generated_marker};
use crate::ir::ConstSpec;
use crate::render::expect_decimal;

const MODULE_DOC: &str = "\
//! Hook API error codes.
//!
//! Upstream: `Xahau/xahaud`, branch `release`, `hook/error.h`, vendored at
//! `crates/hooks-core/vendor/xahaud-hook/error.h`.
//!
//! Every Hook API function returns an `i64`; non-negative values are
//! success payloads (often \"bytes written\"), negative values are one of
//! the error codes below. Kept verbatim (name and value) from the C header.
";

/// The one irregular value in `error.h`: `INVALID_FLOAT` is `-10024`, not
/// the `-24` its position in the sequence would suggest. Both the header
/// and the hand-authored translation call this out explicitly; the
/// generator preserves that note verbatim rather than emitting the default
/// one-line doc for this single item.
fn doc_lines(name: &str) -> Vec<String> {
    if name == "INVALID_FLOAT" {
        vec![
            "C: `INVALID_FLOAT` (error.h) — note the upstream value is `-10024`, not".to_string(),
            "`-24` (kept verbatim; this is not a typo in this translation).".to_string(),
        ]
    } else {
        vec![format!("C: `{name}` (error.h)")]
    }
}

/// Renders `error.rs`'s full contents from `error.h`'s parsed [`ConstSpec`]s.
pub fn generate(error_codes: &[ConstSpec]) -> Result<String> {
    let mut body = String::from("\n");
    for d in error_codes {
        let value = expect_decimal(&d.name, &d.c_expr)?;
        push_const(&mut body, &doc_lines(&d.name), &d.name, "i64", &value);
    }
    Ok(with_generated_marker("error.h", MODULE_DOC) + &body)
}
