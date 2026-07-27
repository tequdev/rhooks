//! Generates `crates/hooks-core/src/tts.rs` from `tts.h`'s parsed
//! [`ConstSpec`]s (`crates/xtask/src/ir.rs`, `hook_api.json`).

use anyhow::Result;

use super::{push_const, with_generated_marker};
use crate::ir::ConstSpec;
use crate::render::expect_decimal;

const MODULE_DOC: &str = "\
//! Transaction type (`ttXXX`) codes.
//!
//! Upstream: `Xahau/xahaud`, branch `release`, `hook/tts.h`, vendored at
//! `crates/hooks-core/vendor/xahaud-hook/tts.h`.
";

/// Renders `tts.rs`'s full contents from `tts.h`'s parsed [`ConstSpec`]s.
pub fn generate(tts: &[ConstSpec]) -> Result<String> {
    let mut body = String::from("\n");
    for d in tts {
        let value = expect_decimal(&d.name, &d.c_expr)?;
        let doc = vec![format!("C: `{}` (tts.h)", d.name)];
        push_const(&mut body, &doc, &d.name, "u16", &value);
    }
    Ok(with_generated_marker("tts.h", MODULE_DOC) + &body)
}
