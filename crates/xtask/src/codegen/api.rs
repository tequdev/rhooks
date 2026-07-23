//! Generates `crates/hooks-core/src/api.rs` from `extern.h`.

use anyhow::Result;

use super::with_generated_marker;
use crate::parse::{ExternFn, c_type_to_rust, scan_extern_fns};

const MODULE_DOC: &str = "\
//! Raw Hook API function declarations.
//!
//! Upstream: `Xahau/xahaud`, branch `release`, `hook/extern.h`, vendored at
//! `crates/hooks-core/vendor/xahaud-hook/extern.h`.
//!
//! This mirrors `extern.h` exactly, in header order: 75 functions total
//! (`_g` plus 74 Hook API functions), all imported from wasm import module
//! `env`. Parameter names and types are kept verbatim (`read_ptr`/`read_len`
//! style `u32`, `i64` returns) so C hook source and this file can be
//! compared line by line.
//!
//! On non-`wasm32` targets (host builds, so hooks-lib and its tests/docs can
//! compile and run) the same signatures are provided as deterministic stub
//! functions that return [`crate::error::NOT_IMPLEMENTED`] (the `_g` stub
//! returns `0`, i.e. \"guard check passed\"). None of the stubs panic.
";

/// The original `extern.h` prototype text, as it appears in the doc comment
/// provenance line (`/// C: \`<this>\` (extern.h)`) — built from the C
/// (not Rust-mapped) types so the doc genuinely quotes the header.
fn c_prototype_text(f: &ExternFn) -> String {
    let params = f
        .params
        .iter()
        .map(|(ty, name)| format!("{ty} {name}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{} {}({params})", f.ret_c_ty, f.name)
}

fn rust_signature(f: &ExternFn) -> Result<(String, String)> {
    let ret_ty = c_type_to_rust(&f.ret_c_ty)?;
    let mut params = Vec::with_capacity(f.params.len());
    for (ty, name) in &f.params {
        params.push(format!("{name}: {}", c_type_to_rust(ty)?));
    }
    Ok((params.join(", "), ret_ty.to_string()))
}

/// clippy's `too_many_arguments` lint fires once a function has more
/// parameters than its threshold; the hand-authored `api.rs` silences it
/// (`#[allow(clippy::too_many_arguments)]`) on every declaration with 7 or
/// more parameters (`sto_emplace` through `util_keylet`) — verified against
/// every one of `extern.h`'s 75 prototypes.
fn needs_too_many_arguments_allow(f: &ExternFn) -> bool {
    f.params.len() >= 7
}

/// Renders `api.rs`'s full contents from `extern.h`'s source text.
pub fn generate(extern_h: &str) -> Result<String> {
    let fns = scan_extern_fns(extern_h)?;

    let mut wasm_block = String::new();
    for f in &fns {
        let (params, ret_ty) = rust_signature(f)?;
        wasm_block.push_str(&format!(
            "    /// C: `{}` (extern.h)\n",
            c_prototype_text(f)
        ));
        if needs_too_many_arguments_allow(f) {
            wasm_block.push_str("    #[allow(clippy::too_many_arguments)]\n");
        }
        wasm_block.push_str(&format!("    pub fn {}({params}) -> {ret_ty};\n\n", f.name));
    }
    // Drop the trailing blank line before the block's closing brace.
    wasm_block.truncate(wasm_block.trim_end_matches('\n').len());
    wasm_block.push('\n');

    let mut stubs = String::new();
    for f in &fns {
        let (params, ret_ty) = rust_signature(f)?;
        let body = if f.name == "_g" {
            "0"
        } else {
            "NOT_IMPLEMENTED"
        };
        stubs.push_str(&format!(
            "    /// C: `{}` (extern.h) — host stub\n",
            c_prototype_text(f)
        ));
        stubs.push_str(&format!(
            "    pub unsafe fn {}({params}) -> {ret_ty} {{\n        {body}\n    }}\n\n",
            f.name
        ));
    }
    stubs.truncate(stubs.trim_end_matches('\n').len());
    stubs.push('\n');

    let mut out = with_generated_marker("extern.h", MODULE_DOC);
    out.push('\n');
    out.push_str("// The Hook API import block, exactly as declared in `extern.h`.\n");
    out.push_str("#[cfg(target_arch = \"wasm32\")]\n");
    out.push_str("#[link(wasm_import_module = \"env\")]\n");
    out.push_str("unsafe extern \"C\" {\n");
    out.push_str(&wasm_block);
    out.push_str("}\n");
    out.push('\n');
    out.push_str("/// Non-wasm host stubs mirroring the `extern.h` signatures exactly, so\n");
    out.push_str("/// hooks-lib and its docs/tests compile and run on the host. Every stub is a\n");
    out.push_str("/// deterministic, non-panicking placeholder: it returns\n");
    out.push_str("/// [`crate::error::NOT_IMPLEMENTED`] (the `_g` stub returns `0`, meaning\n");
    out.push_str("/// \"guard check passed\").\n");
    out.push_str("#[cfg(not(target_arch = \"wasm32\"))]\n");
    out.push_str("#[allow(\n");
    out.push_str("    missing_docs,\n");
    out.push_str("    clippy::missing_safety_doc,\n");
    out.push_str("    unused_variables,\n");
    out.push_str("    clippy::too_many_arguments\n");
    out.push_str(")]\n");
    out.push_str("mod host_stubs {\n");
    out.push_str("    use crate::error::NOT_IMPLEMENTED;\n");
    out.push('\n');
    out.push_str(&stubs);
    out.push_str("}\n");
    out.push('\n');
    out.push_str("#[cfg(not(target_arch = \"wasm32\"))]\n");
    out.push_str("pub use host_stubs::*;\n");

    Ok(out)
}
