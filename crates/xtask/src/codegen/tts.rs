//! Generates `crates/hooks-core/src/tts.rs` from `tts.h`.

use anyhow::Result;

use super::{push_const, with_generated_marker};
use crate::parse::scan_defines;
use crate::render::expect_decimal;

const MODULE_DOC: &str = "\
//! Transaction type (`ttXXX`) codes.
//!
//! Upstream: `Xahau/xahaud`, branch `release`, `hook/tts.h`, vendored at
//! `crates/hooks-core/vendor/xahaud-hook/tts.h`.
";

/// Renders `tts.rs`'s full contents from `tts.h`'s source text.
pub fn generate(tts_h: &str) -> Result<String> {
    let defines = scan_defines(tts_h);
    let mut body = String::from("\n");
    for d in &defines {
        let value = expect_decimal(&d.name, &d.value)?;
        let doc = vec![format!("C: `{}` (tts.h)", d.name)];
        push_const(&mut body, &doc, &d.name, "u32", &value);
    }
    Ok(with_generated_marker("tts.h", MODULE_DOC) + &body)
}
