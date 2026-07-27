//! `#[derive(HookData)]` — backs `hooks_lib::HookData`.
//!
//! Turns a plain, fixed-size, named-field struct into a fixed-offset,
//! zero-cost `hooks_lib::convert::ToBytes`/`FromBytes`/`FixedRead` triple, so
//! the struct can be used directly as a hook-state/`otxn_param`/`hook_param`
//! *value* — or, via `hooks_lib::state`'s blanket `StateKeyEncode` impl over
//! any `ToBytes` type, as a state *key* too (see `hooks_lib::state`'s module
//! doc comment). See `hooks_lib::HookData`'s doc comment (the public-facing
//! re-export site) for the full user-facing writeup, grammar, and worked/
//! compile-fail examples — this module only implements the codegen.
//!
//! # Why hand-rolled, not `syn`/`quote`
//!
//! Same reasoning as the rest of this crate (see the crate doc comment):
//! this derive only ever needs to recognize one shape (a named-field
//! struct, each field a bare `name: Type` pair, `Type` being a path or a
//! `[u8; N]` array) — never a general Rust-item/type parser. `syn`+`quote`'s
//! compile cost would be paid on every build of every hook crate for a
//! job this small, token-shape-matching pass handles directly.
//!
//! # Codegen strategy
//!
//! Every field's byte width is that field's own
//! `<FieldType as ToBytes>::MAX_LEN` — an associated-const expression, not a
//! value this macro can compute (it only ever sees a field's type as
//! syntax, e.g. the text `AccountId` or `[u8; 20]`, never its resolved
//! `MAX_LEN`, which may live in a crate this macro cannot see). So instead
//! of baking in literal numeric offsets, the generated code computes a
//! chain of `const __OFF_N: usize = __OFF_{N-1} + <FieldTypeN as
//! ToBytes>::MAX_LEN;` declarations — one per field boundary, entirely
//! compile-time — and every field read/write uses `__dst[__OFF_i..__OFF_{i+1}]`
//! against those consts. Because every offset is a compile-time constant
//! (never a runtime-computed length), and every per-field copy delegates to
//! that field's own already-optimized `ToBytes::write`/`FromBytes::read`
//! (itself following the same convention, all the way down through nested
//! `#[derive(HookData)]` types), the result is the same "unrolled, fixed
//! offset" shape `hooks-lib` already hand-writes elsewhere (see
//! `hooks_lib::txn::codec`'s `write_field_header`/`write_const_bytes` and
//! `txn_template!`'s generated setters, which use the identical
//! `#[allow(clippy::indexing_slicing)]`-annotated fixed-offset-range pattern
//! for the same reason: proven in-bounds by construction, not by a runtime
//! check clippy can see).
//!
//! # Why the generated code hardcodes `::hooks_lib::...` paths
//!
//! This derive is re-exported as `hooks_lib::HookData`, so every crate that
//! can invoke it already depends on `hooks-lib` under that exact name (Cargo
//! normalizes the hyphen to an underscore) — the generated code can
//! therefore reference `::hooks_lib::convert::{ToBytes, FromBytes,
//! FixedRead}` and `::hooks_lib::error::{HookError, Result}` as absolute
//! paths unconditionally, without requiring the invoking module to have
//! those names in scope via `use` (unlike relying on `hooks_lib::prelude::*`
//! already being imported, which every example happens to do but which this
//! derive does not assume).

use crate::err;
use proc_macro::{Delimiter, Spacing, Span, TokenStream, TokenTree};
use std::iter::Peekable;

/// One `name: Type` field, as captured from the input tokens.
struct FieldShape {
    /// The field's name, verbatim.
    name: String,
    /// The field's type, reconstructed as source text (see
    /// [`tokens_to_string`]) — never type-checked by this macro itself;
    /// a type that doesn't implement `ToBytes`/`FromBytes` surfaces as an
    /// ordinary rustc trait-bound error against the generated impl, not a
    /// diagnostic this macro produces directly.
    ty: String,
}

/// A named-field struct's shape, as captured from the input tokens.
struct StructShape {
    /// The struct's name, verbatim.
    name: String,
    /// Span of the struct's name, used to anchor struct-level errors (e.g.
    /// "must have at least one field").
    name_span: Span,
    /// Fields in declaration order — the order every generated offset,
    /// the layout doc table, and the struct literal in `FromBytes::read`
    /// all follow.
    fields: Vec<FieldShape>,
}

/// Entry point invoked by `#[proc_macro_derive(HookData)]` in `lib.rs`.
pub fn derive(input: TokenStream) -> TokenStream {
    match parse_struct(input) {
        Ok(shape) => generate(&shape),
        Err(e) => e,
    }
}

/// Advances past any leading `#[...]` attributes (including doc comments,
/// which the compiler already desugars to `#[doc = "..."]` by the time this
/// macro sees them). Input to a derive macro is always a syntactically
/// valid item (rustc parses it before invoking the derive), so this never
/// needs to report an error — an attribute here is always exactly `#`
/// followed by a bracketed group.
fn skip_attrs(iter: &mut Peekable<impl Iterator<Item = TokenTree>>) {
    loop {
        match iter.peek() {
            Some(TokenTree::Punct(p)) if p.as_char() == '#' => {
                iter.next();
                iter.next();
            }
            _ => break,
        }
    }
}

/// Advances past an optional leading `pub`/`pub(...)` visibility.
fn skip_vis(iter: &mut Peekable<impl Iterator<Item = TokenTree>>) {
    if let Some(TokenTree::Ident(id)) = iter.peek() {
        if id.to_string() == "pub" {
            iter.next();
            if let Some(TokenTree::Group(g)) = iter.peek() {
                if g.delimiter() == Delimiter::Parenthesis {
                    iter.next();
                }
            }
        }
    }
}

/// Parses the derive input into a [`StructShape`], or a `compile_error!`
/// `TokenStream` describing the first shape violation found.
fn parse_struct(input: TokenStream) -> Result<StructShape, TokenStream> {
    let mut iter = input.into_iter().peekable();

    skip_attrs(&mut iter);
    skip_vis(&mut iter);

    match iter.next() {
        Some(TokenTree::Ident(id)) if id.to_string() == "struct" => {}
        Some(TokenTree::Ident(id)) if id.to_string() == "enum" => {
            return Err(err(
                id.span(),
                "HookData can only be derived for a struct, not an enum",
            ));
        }
        Some(TokenTree::Ident(id)) if id.to_string() == "union" => {
            return Err(err(
                id.span(),
                "HookData can only be derived for a struct, not a union",
            ));
        }
        Some(other) => return Err(err(other.span(), "HookData: expected a struct")),
        None => return Err(err(Span::call_site(), "HookData: expected a struct")),
    }

    let name_id = match iter.next() {
        Some(TokenTree::Ident(id)) => id,
        Some(other) => return Err(err(other.span(), "HookData: expected a struct name")),
        None => return Err(err(Span::call_site(), "HookData: expected a struct name")),
    };
    let name_span = name_id.span();
    let name = name_id.to_string();

    if let Some(TokenTree::Punct(p)) = iter.peek() {
        if p.as_char() == '<' {
            return Err(err(p.span(), "HookData does not support generic structs"));
        }
    }

    match iter.next() {
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
            let fields = parse_fields(g.stream())?;
            if fields.is_empty() {
                return Err(err(
                    name_span,
                    "HookData: struct must have at least one field",
                ));
            }
            Ok(StructShape {
                name,
                name_span,
                fields,
            })
        }
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis => Err(err(
            g.span(),
            "HookData does not support tuple structs — use named fields",
        )),
        Some(TokenTree::Punct(p)) if p.as_char() == ';' => Err(err(
            p.span(),
            "HookData does not support unit structs — a state key/value needs at least one field",
        )),
        Some(other) => Err(err(
            other.span(),
            "HookData: expected a `{ .. }` field list",
        )),
        None => Err(err(name_span, "HookData: expected a `{ .. }` field list")),
    }
}

/// Parses a brace group's inner stream into an ordered list of fields.
/// Every field must be `name: Type` (attributes and a leading `pub`/
/// `pub(...)` are skipped, not validated); `Type` is captured as raw tokens
/// up to the next top-level comma, so a bracketed array type (`[u8; 20]`)
/// or a multi-segment path (`crate::types::AccountId`) both work — nothing
/// deeper than one field-list nesting level is inspected.
fn parse_fields(stream: TokenStream) -> Result<Vec<FieldShape>, TokenStream> {
    let mut iter = stream.into_iter().peekable();
    let mut fields = Vec::new();

    while iter.peek().is_some() {
        skip_attrs(&mut iter);
        skip_vis(&mut iter);

        if iter.peek().is_none() {
            break;
        }

        let name_id = match iter.next() {
            Some(TokenTree::Ident(id)) => id,
            Some(other) => return Err(err(other.span(), "HookData: expected a field name")),
            None => break,
        };
        let field_span = name_id.span();
        let field_name = name_id.to_string();

        match iter.next() {
            Some(TokenTree::Punct(p)) if p.as_char() == ':' => {}
            Some(other) => {
                return Err(err(other.span(), "HookData: expected `:` after field name"));
            }
            None => {
                return Err(err(field_span, "HookData: expected `:` after field name"));
            }
        }

        let mut ty_tokens: Vec<TokenTree> = Vec::new();
        loop {
            match iter.peek() {
                Some(TokenTree::Punct(p)) if p.as_char() == ',' => {
                    iter.next();
                    break;
                }
                Some(_) => {
                    if let Some(tt) = iter.next() {
                        ty_tokens.push(tt);
                    }
                }
                None => break,
            }
        }
        if ty_tokens.is_empty() {
            return Err(err(field_span, "HookData: expected a field type"));
        }

        fields.push(FieldShape {
            name: field_name,
            ty: tokens_to_string(&ty_tokens),
        });
    }

    Ok(fields)
}

/// Reconstructs a type's source text from its captured tokens, preserving
/// exactly the adjacency `hooks-macros`' own tokenizer already recorded via
/// each `Punct`'s [`Spacing`] — so a multi-token compound like the `::` in
/// `crate::types::AccountId` round-trips as `::` (no space, which Rust's
/// path grammar requires) rather than `: :` (two independent colons, a
/// parse error in path position). A space is inserted before every other
/// token boundary; this is always syntactically safe (it can only ever
/// separate two tokens that were already distinct, never accidentally glue
/// two identifiers/literals into one).
fn tokens_to_string(tokens: &[TokenTree]) -> String {
    let mut out = String::new();
    let mut prev_joint = false;
    for tt in tokens {
        if !out.is_empty() && !prev_joint {
            out.push(' ');
        }
        out.push_str(&tt.to_string());
        prev_joint = matches!(tt, TokenTree::Punct(p) if p.spacing() == Spacing::Joint);
    }
    out
}

/// Builds a rustdoc table (declaration order, field name, field type) for
/// the generated `LEN` const. Deliberately does not print numeric offsets —
/// this macro only ever sees a field's type as syntax, never its resolved
/// `ToBytes::MAX_LEN`, so the concrete byte offsets are a compile-time fact
/// this comment can describe but not compute.
fn layout_table_doc(shape: &StructShape) -> String {
    let mut s = String::new();
    s.push_str("/// Total encoded length in bytes: [`hooks_lib::convert::ToBytes::MAX_LEN`].\n");
    s.push_str("///\n");
    s.push_str("/// Generated by `#[derive(HookData)]`. Fields are encoded back-to-back in\n");
    s.push_str("/// declaration order, each contributing exactly its own `ToBytes::MAX_LEN`\n");
    s.push_str("/// bytes — no padding between fields:\n");
    s.push_str("///\n");
    s.push_str("/// | # | field | type |\n");
    s.push_str("/// |---|---|---|\n");
    for (i, f) in shape.fields.iter().enumerate() {
        s.push_str(&format!(
            "/// | {i} | `{name}` | `{ty}` |\n",
            i = i,
            name = f.name,
            ty = f.ty,
        ));
    }
    s
}

/// Generates the `ToBytes`/`FromBytes`/`FixedRead` impls plus the inherent
/// `LEN` const, for an already-validated [`StructShape`].
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

    let mut read_body = String::new();
    for (i, f) in shape.fields.iter().enumerate() {
        read_body.push_str(&format!(
            "{field}: <{ty} as ::hooks_lib::convert::FromBytes>::read(&__src[__OFF_{i}..__OFF_{next}])?,\n",
            field = f.name,
            ty = f.ty,
            i = i,
            next = i.wrapping_add(1),
        ));
    }

    let layout_doc = layout_table_doc(shape);

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
impl ::hooks_lib::convert::FromBytes for {name} {{
    #[inline(always)]
    #[allow(clippy::indexing_slicing)] // same fixed compile-time offsets as the `ToBytes::write` impl above\n\
    fn read(buf: &[u8]) -> ::hooks_lib::error::Result<Self> {{
        let __src = buf.get(..<Self as ::hooks_lib::convert::ToBytes>::MAX_LEN)
            .ok_or(::hooks_lib::error::HookError::TooSmall)?;
        {offset_consts}
        ::core::result::Result::Ok(Self {{
            {read_body}
        }})
    }}
}}

#[automatically_derived]
impl ::hooks_lib::convert::FixedRead for {name} {{
    #[inline(always)]
    fn read_exact(
        read: impl FnOnce(&mut [u8]) -> ::hooks_lib::error::Result<usize>,
    ) -> ::hooks_lib::error::Result<Self> {{
        let mut __buf = [0u8; <Self as ::hooks_lib::convert::ToBytes>::MAX_LEN];
        let __written = read(&mut __buf)?;
        if __written == <Self as ::hooks_lib::convert::ToBytes>::MAX_LEN {{
            <Self as ::hooks_lib::convert::FromBytes>::read(&__buf)
        }} else {{
            ::core::result::Result::Err(::hooks_lib::error::HookError::TooSmall)
        }}
    }}
}}

impl {name} {{
    {layout_doc}
    pub const LEN: usize = <Self as ::hooks_lib::convert::ToBytes>::MAX_LEN;
}}
",
        name = name,
        max_len_expr = max_len_expr,
        offset_consts = offset_consts,
        write_body = write_body,
        read_body = read_body,
        layout_doc = layout_doc,
    );

    match src.parse::<TokenStream>() {
        Ok(ts) => ts,
        Err(_) => err(
            shape.name_span,
            "hooks-macros: internal HookData codegen failed to parse",
        ),
    }
}
