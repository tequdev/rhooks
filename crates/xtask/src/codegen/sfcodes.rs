//! Generates `crates/hooks-core/src/sfcodes.rs` from `sfcodes.h`.

use anyhow::Result;

use super::{push_const, with_generated_marker};
use crate::parse::scan_defines;
use crate::render::render_shift_add;

const MODULE_DOC: &str = "\
//! Serialized field (`sfXxx`) codes.
//!
//! Upstream: `Xahau/xahaud`, branch `release`, `hook/sfcodes.h`, vendored at
//! `crates/hooks-core/vendor/xahaud-hook/sfcodes.h`.
//!
//! Each code packs a type code and a field index: `(type << 16) + index`,
//! mirrored verbatim from the header (325 fields).
";

/// Renders `sfcodes.rs`'s full contents from `sfcodes.h`'s source text.
pub fn generate(sfcodes_h: &str) -> Result<String> {
    let defines = scan_defines(sfcodes_h);
    let mut body = String::from("\n");
    for d in &defines {
        let value = render_shift_add(&d.value)?;
        let doc = vec![format!("C: `{}` (sfcodes.h)", d.name)];
        push_const(&mut body, &doc, &d.name, "u32", &value);
    }
    Ok(with_generated_marker("sfcodes.h", MODULE_DOC) + &body)
}
