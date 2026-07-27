//! `#[derive(ParamName)]` — backs `hooks_lib::ParamName`.
//!
//! Turns a plain, fixed-size, named-field struct into a fixed-offset,
//! zero-cost `hooks_lib::convert::ToBytes` impl for use as a **composite
//! Hook API parameter name** — the `Name` in
//! `hooks_lib::convert::TypedParam`'s `type Name: ToBytes` (see
//! `hooks_lib::hook_parameter!`/`hooks_lib::otxn_parameter!`'s composite
//! form). See `hooks_lib::ParamName`'s doc comment (the public-facing
//! re-export site) for the full user-facing writeup, grammar, and
//! worked/compile-fail examples — this module only implements the codegen.
//!
//! # Why a separate derive, not `#[derive(HookData)]`?
//!
//! A hook-state key/value (`HookKey`/`HookData`) and a Hook API parameter
//! *name* share the same "fixed-offset struct" shape, but are genuinely
//! different concepts, and `ParamName` is deliberately narrower:
//!
//! - A parameter name is only ever **written** — handed to
//!   `hook_param`/`otxn_param` to locate a value, never read back and
//!   decoded as itself. So `ParamName` generates only
//!   `hooks_lib::convert::ToBytes` (the encoding half) — no `FromBytes`,
//!   no `FixedRead`, no inherent `LEN` const or layout doc table. Trying
//!   to read a `#[derive(ParamName)]` type back as a value (or use it
//!   where `hook_param_typed`/`otxn_param_typed` expect a value type) fails to
//!   compile with an ordinary rustc trait-bound error naming the missing
//!   trait — the same "don't implement our own type checker, let rustc's
//!   error do the work" convention `HookData` already follows for a field
//!   of the wrong type. The value-side counterpart is
//!   `#[derive(ParamValue)]` (see [`crate::param_value`]).
//! - A Hook API parameter name has its own length bound the Hook API
//!   itself enforces (`hook_api.h`: 1 to 32 bytes — see
//!   `hooks_lib::convert::PARAM_NAME_MAX_LEN`), distinct from a hook state
//!   key's *fixed* 32 bytes (always zero-padded — see
//!   [`crate::hook_key`]) or a hook state *value*'s lack of any size cap
//!   at all. `ParamName` bakes this in as an unconditional compile-time
//!   assert generated alongside the `ToBytes` impl — a
//!   `#[derive(ParamName)]` struct that encodes to 0 or to 33+ bytes fails
//!   to compile *at its own definition*, not only later when something
//!   tries to actually use it as a parameter name.
//!
//! See `hooks_lib::HookData`'s/`hooks_lib::HookKey`'s doc comments for the
//! reciprocal note (use those for hook-state values/keys, `ParamName` for
//! parameter names, `ParamValue` for parameter values).
//!
//! # Why hand-rolled, not `syn`/`quote`; codegen strategy
//!
//! Identical rationale to [`crate::hook_data`] (see that module's doc
//! comment) — struct-shape parsing is shared via [`crate::shape`]; only the
//! `ToBytes`-only-plus-length-assert generation differs.

use crate::err;
use crate::shape::{StructShape, parse_struct};
use proc_macro::TokenStream;

/// Entry point invoked by `#[proc_macro_derive(ParamName)]` in `lib.rs`.
pub fn derive(input: TokenStream) -> TokenStream {
    match parse_struct(input, "ParamName") {
        Ok(shape) => generate(&shape),
        Err(e) => e,
    }
}

/// Generates the `ToBytes` impl plus the 1–32-byte compile-time length
/// assert, for an already-validated [`StructShape`]. Deliberately does
/// *not* generate `FromBytes`/`FixedRead`/an inherent `LEN` const — see
/// this module's doc comment for why.
fn generate(shape: &StructShape) -> TokenStream {
    let name = &shape.name;

    let mut max_len_expr = String::from("0usize");
    for f in &shape.fields {
        max_len_expr.push_str(&format!(
            " + <{ty} as ::hooks_lib::convert::ToBytes>::MAX_LEN",
            ty = f.ty
        ));
    }

    let mut offset_consts = String::from("const __OFF_0: usize = 0usize;\n");
    for (i, f) in shape.fields.iter().enumerate() {
        offset_consts.push_str(&format!(
            "const __OFF_{next}: usize = __OFF_{i} + <{ty} as ::hooks_lib::convert::ToBytes>::MAX_LEN;\n",
            next = i.wrapping_add(1),
            i = i,
            ty = f.ty,
        ));
    }

    let mut write_body = String::new();
    for (i, f) in shape.fields.iter().enumerate() {
        write_body.push_str(&format!(
            "let _ = ::hooks_lib::convert::ToBytes::write(&self.{field}, &mut __dst[__OFF_{i}..__OFF_{next}]);\n",
            field = f.name,
            i = i,
            next = i.wrapping_add(1),
        ));
    }

    let src = format!(
        "
#[automatically_derived]
impl ::hooks_lib::convert::ToBytes for {name} {{
    const MAX_LEN: usize = {max_len_expr};

    #[inline(always)]
    #[allow(clippy::indexing_slicing)] // fixed, compile-time field offsets (see __OFF_* below); `__dst` was already proven to have exactly `MAX_LEN` bytes by the `get_mut(..MAX_LEN)` check\n\
    fn write(&self, buf: &mut [u8]) -> usize {{
        match buf.get_mut(..<Self as ::hooks_lib::convert::ToBytes>::MAX_LEN) {{
            ::core::option::Option::Some(__dst) => {{
                {offset_consts}
                {write_body}
                <Self as ::hooks_lib::convert::ToBytes>::MAX_LEN
            }}
            ::core::option::Option::None => 0,
        }}
    }}
}}

#[automatically_derived]
const _: () = {{
    assert!(
        <{name} as ::hooks_lib::convert::ToBytes>::MAX_LEN >= 1,
        \"hooks-macros: #[derive(ParamName)] struct must encode to at least 1 byte (the Hook API's parameter-name lower bound)\"
    );
    assert!(
        <{name} as ::hooks_lib::convert::ToBytes>::MAX_LEN <= ::hooks_lib::convert::PARAM_NAME_MAX_LEN,
        \"hooks-macros: #[derive(ParamName)] struct exceeds the Hook API's 32-byte parameter-name upper bound\"
    );
}};
",
        name = name,
        max_len_expr = max_len_expr,
        offset_consts = offset_consts,
        write_body = write_body,
    );

    match src.parse::<TokenStream>() {
        Ok(ts) => ts,
        Err(_) => err(
            shape.name_span,
            "hooks-macros: internal ParamName codegen failed to parse",
        ),
    }
}
