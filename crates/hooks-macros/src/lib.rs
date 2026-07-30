//! `hooks-macros` — proc-macro support for `hooks-lib`.
//!
//! Two independent pieces of functionality live here:
//!
//! - [`macro@hook`] / [`macro@cbak`] — attribute macros that turn a plain,
//!   argument-less `fn name() -> i64` into the exact wasm export shape the
//!   Hook host requires (`#[unsafe(no_mangle)] pub extern "C" fn hook(
//!   _reserved: u32) -> i64`), so hook authors never hand-write the export
//!   boilerplate (or its `unsafe`) themselves. Re-exported from `hooks-lib`
//!   as `hooks_lib::hook`/`hooks_lib::cbak` — hook authors are not expected
//!   to depend on this crate directly.
//! - [`paste`] — a minimal, purpose-built identifier-concatenation macro
//!   (a tiny replacement for the `paste` crate, covering exactly the one
//!   pattern `hooks-lib`'s `txn_template!` needs) that lets `txn_template!`
//!   synthesize `set_<field>` setter names on **stable** Rust. It replaces
//!   nightly's `${concat(set_, $field)}` metavariable expression, which
//!   `txn_template!` used before this crate existed. `#[doc(hidden)]` and
//!   re-exported from `hooks-lib` as `hooks_lib::__paste` — internal use
//!   only, not part of the public API.
//! - Four fixed-offset struct derives, split by role rather than one
//!   derive covering everything (see each module's own doc comment for the
//!   full rationale) — all four share struct-shape parsing via
//!   [`shape`], differing only in which `hooks_lib::convert` impls they
//!   generate:
//!     - [`macro@HookKey`] (see [`hook_key`]) — a hook-**state key**:
//!       `ToBytes` plus an explicit `hooks_lib::state::StateKeyEncode`
//!       impl (zero-padded to 32 bytes, checked at derive time). Re-exported
//!       as `hooks_lib::HookKey`.
//!     - [`macro@HookData`] (see [`hook_data`]) — a hook-**state value**:
//!       the full `ToBytes`/`FromBytes`/`FixedRead` triple. Re-exported as
//!       `hooks_lib::HookData`.
//!     - [`macro@ParamName`] (see [`param_name`]) — a composite **Hook API
//!       parameter name**: write-only `ToBytes`, with the Hook API's own
//!       1–32-byte parameter-name bound checked at derive time.
//!       Re-exported as `hooks_lib::ParamName`.
//!     - [`macro@ParamValue`] (see [`param_value`]) — a **Hook API
//!       parameter value**: read-only `FromBytes`/`FixedRead`. Re-exported
//!       as `hooks_lib::ParamValue`.
//!
//!   See each re-export's doc comment (in `hooks-lib`) for the full
//!   user-facing writeup (grammar, examples, zero-cost codegen shape,
//!   compile-fail cases); hook authors are not expected to depend on this
//!   crate directly.
//! - [`account_id`] — decodes a classic XRPL/Xahau r-address (base58check
//!   string) into an `AccountId` literal **entirely at compile time**, host
//!   side, inside this macro. The expansion is a plain `AccountId([u8;
//!   20])` literal, so it costs the compiled Hook wasm binary nothing extra
//!   — no decode logic ships in the wasm at all. Implemented with
//!   hand-written base58 decode ([`base58`]) + SHA-256 ([`sha256`]) rather
//!   than adding `bs58`/`sha2` dependencies, for the same "don't add
//!   mandatory build-time weight to every Hook crate" reasoning below.
//!   Re-exported from `hooks-lib` as `hooks_lib::account_id`, which is
//!   where the full usage docs live.
//!
//! # Why hand-rolled `proc_macro`, not `syn`/`quote`
//!
//! Every macro here only ever needs to recognize a handful of token shapes
//! (a no-argument, `i64`-returning `fn` item; a `[< ident ident >]` splice
//! marker; a named-field struct whose fields are `name: Type` pairs) —
//! never a general Rust-item parser. This crate's own build output is host
//! tooling, not a wasm Hook artifact, so the byte-size budget that governs
//! `hooks-lib`/`hooks-core` doesn't apply here directly — but it still
//! governs indirectly, because `hooks-macros` is a mandatory build-time
//! dependency of *every* hook crate: `syn`+`quote`'s (non-trivial,
//! transitively-heavy) compile cost would be paid on every `cargo
//! build`/`cargo check` of every hook, for a token-shape-matching job
//! simple enough for direct `proc_macro::TokenStream` walking. A std-only
//! `proc_macro` crate with zero dependencies is the cheaper choice given
//! how small and stable those shapes are.
//!
//! The same reasoning applies to [`account_id`]'s own dependencies: rather
//! than adding `bs58` (base58 decode) and `sha2` (checksum verification) —
//! each individually small, but each still a mandatory build-time cost paid
//! by every hook crate, for algorithms that are, respectively, ~50 and
//! ~100 lines of straightforward, well-specified code — both are
//! hand-written here (see [`base58`] and [`sha256`]).
//!
//! This crate relaxes `clippy::arithmetic_side_effects` relative to the
//! workspace default, for the same reason `hooks-build` does (see its own
//! crate doc comment): unlike `hooks-core`/`hooks-lib`, which run *inside*
//! the wasm guest, `hooks-macros` is ordinary host-side proc-macro tooling.
//! Its arithmetic (SHA-256's fixed 64-round message schedule/compression
//! loop, base58's per-digit big-integer carry propagation) is structurally
//! bounded by small, fixed constants — nowhere near overflow — so a panic
//! here would mean a genuine bug in this crate, not a reachable
//! user-input-driven failure mode (malformed r-address *input* is instead
//! turned into a `compile_error!` via [`base58::DecodeError`], never a
//! panic — see [`account_id`]). `clippy::unwrap_used`, `clippy::expect_used`,
//! `clippy::panic`, and `clippy::indexing_slicing` remain denied, as
//! inherited from the workspace; indexing that genuinely cannot go out of
//! bounds is instead allowed function-by-function, with a justification
//! comment, matching `hooks-lib`'s own convention (see e.g. `macros.rs`'s
//! `padded_bytes`).
#![allow(clippy::arithmetic_side_effects)]

mod base58;
mod sha256;

use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};

mod decl_pair;
mod hook_data;
mod hook_key;
mod param_name;
mod param_value;
mod shape;

/// Turns a plain `fn name() -> i64 { .. }` into the Hook host's required
/// `hook` export.
///
/// Expands to the original function (unchanged) plus:
///
/// ```ignore
/// #[unsafe(no_mangle)]
/// pub extern "C" fn hook(_reserved: u32) -> i64 {
///     name()
/// }
/// ```
///
/// # Requirements
///
/// The annotated item must be a plain (`async`/`unsafe`/`const`/`extern`
/// modifiers not allowed, no generics, no `where` clause) `fn` that takes no
/// arguments and returns `i64`; `#[hook]` itself takes no arguments. Any
/// violation is reported as a `compile_error!` pointing at the offending
/// token, not a panic.
///
/// # Examples
///
/// ```
/// use hooks_macros::hook;
///
/// #[hook]
/// fn my_hook() -> i64 {
///     0
/// }
/// ```
///
/// Hook authors do not depend on `hooks-macros` directly in practice — see
/// `hooks_lib::hook`, which re-exports this and is what hook crates
/// actually import.
#[proc_macro_attribute]
pub fn hook(attr: TokenStream, item: TokenStream) -> TokenStream {
    entry_point("hook", attr, item)
}

/// Like [`macro@hook`], but exports `cbak` instead of `hook` — for the
/// optional callback a Hook module can export, invoked when a transaction it
/// previously emitted settles. See [`macro@hook`] for the exact requirements
/// and generated shape (identical, save for the export name).
#[proc_macro_attribute]
pub fn cbak(attr: TokenStream, item: TokenStream) -> TokenStream {
    entry_point("cbak", attr, item)
}

/// Derives `hooks_lib::convert::ToBytes` plus an explicit
/// `hooks_lib::state::StateKeyEncode` impl for a fixed-size, named-field
/// struct used as a **composite hook-state key** — see
/// `hooks_lib::HookKey`'s doc comment (the public-facing re-export hook
/// authors actually use) for the full writeup. Implemented in
/// [`hook_key`]; kept as a thin `#[proc_macro_derive]` entry point here,
/// mirroring [`hook`]/[`cbak`]'s split between the `#[proc_macro...]`
/// entry point and its implementation.
#[proc_macro_derive(HookKey)]
pub fn derive_hook_key(input: TokenStream) -> TokenStream {
    hook_key::derive(input)
}

/// Derives `hooks_lib::convert::ToBytes`/`FromBytes`/`FixedRead` for a
/// fixed-size, named-field struct used as a **hook-state value** — see
/// `hooks_lib::HookData`'s doc comment (the public-facing re-export hook
/// authors actually use) for the full writeup. Implemented in
/// [`hook_data`]; kept as a thin `#[proc_macro_derive]` entry point here,
/// mirroring [`hook`]/[`cbak`]'s split between the `#[proc_macro...]`
/// entry point and its implementation.
#[proc_macro_derive(HookData)]
pub fn derive_hook_data(input: TokenStream) -> TokenStream {
    hook_data::derive(input)
}

/// Derives `hooks_lib::convert::ToBytes` (only — no `FromBytes`/`FixedRead`)
/// for a fixed-size, named-field struct used as a **composite Hook API
/// parameter name** — see `hooks_lib::ParamName`'s doc comment (the
/// public-facing re-export hook authors actually use) for the full
/// writeup. Implemented in [`param_name`]; kept as a thin
/// `#[proc_macro_derive]` entry point here, mirroring [`macro@HookData`]'s
/// split between the `#[proc_macro...]` entry point and its implementation.
#[proc_macro_derive(ParamName)]
pub fn derive_param_name(input: TokenStream) -> TokenStream {
    param_name::derive(input)
}

/// Derives `hooks_lib::convert::FromBytes`/`FixedRead` (only — no
/// `ToBytes`) for a fixed-size, named-field struct used as a **Hook API
/// parameter value** — see `hooks_lib::ParamValue`'s doc comment (the
/// public-facing re-export hook authors actually use) for the full
/// writeup. Implemented in [`param_value`]; kept as a thin
/// `#[proc_macro_derive]` entry point here, mirroring [`macro@HookData`]'s
/// split between the `#[proc_macro...]` entry point and its implementation.
#[proc_macro_derive(ParamValue)]
pub fn derive_param_value(input: TokenStream) -> TokenStream {
    param_value::derive(input)
}

/// Declares a hook-state key/value pairing — see `hooks_lib::hook_state!`'s
/// doc comment (the public-facing re-export site) for the full grammar
/// staircase (Forms 1–4, the `existing` keyword form and the
/// backward-compatible existing-types form), the accessors every declaring
/// form generates, the optional instance binder, and worked examples. Implemented in [`decl_pair`]; kept as a thin
/// `#[proc_macro]` entry point here, mirroring [`hook`]/[`cbak`]'s split
/// between the `#[proc_macro...]` entry point and its implementation.
#[proc_macro]
pub fn hook_state(input: TokenStream) -> TokenStream {
    decl_pair::expand(input, decl_pair::Role::State)
}

/// Declares a Hook API parameter name/value pairing, read via
/// `hook_param_typed` — see `hooks_lib::hook_parameter!`'s doc comment (the
/// public-facing re-export site) for the full grammar staircase and worked
/// examples. Implemented in [`decl_pair`]; kept as a thin `#[proc_macro]`
/// entry point here, mirroring [`hook`]/[`cbak`]'s split between the
/// `#[proc_macro...]` entry point and its implementation.
#[proc_macro]
pub fn hook_parameter(input: TokenStream) -> TokenStream {
    decl_pair::expand(input, decl_pair::Role::HookParam)
}

/// Identical grammar to [`macro@hook_parameter`], targeting
/// `otxn_param_typed` (a parameter attached to the *originating
/// transaction*) instead of `hook_param_typed` — see
/// `hooks_lib::otxn_parameter!`'s doc comment for the full writeup.
#[proc_macro]
pub fn otxn_parameter(input: TokenStream) -> TokenStream {
    decl_pair::expand(input, decl_pair::Role::OtxnParam)
}

/// Decodes a classic XRPL/Xahau r-address (base58check string literal,
/// e.g. `account_id!("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh")`) into an
/// `::hooks_lib::types::AccountId` literal, entirely at compile time (host
/// side, inside this macro — see [`base58::decode`]/[`sha256::sha256`]).
///
/// The full usage docs — worked examples, known-address pairs, and the
/// `compile_fail` cases — live at `hooks_lib::account_id`'s doc comment
/// (this crate's re-export point), since that's what Hook authors actually
/// depend on and read docs for.
///
/// Expects exactly one token: a string literal. Anything else (missing,
/// extra tokens, a non-string literal) or a string that fails to decode is
/// reported as a `compile_error!` at the offending token, never a panic.
#[proc_macro]
pub fn account_id(input: TokenStream) -> TokenStream {
    let mut iter = input.into_iter();

    let literal = match iter.next() {
        Some(TokenTree::Literal(lit)) => lit,
        Some(other) => {
            return err(
                other.span(),
                "account_id! expects a single string literal, e.g. account_id!(\"r...\")",
            );
        }
        None => {
            return err(
                Span::call_site(),
                "account_id! expects a single string literal, e.g. account_id!(\"r...\")",
            );
        }
    };

    if let Some(extra) = iter.next() {
        return err(
            extra.span(),
            "account_id! expects a single string literal, e.g. account_id!(\"r...\") \
             (unexpected extra tokens)",
        );
    }

    let span = literal.span();
    let address = match unquote_str(&literal.to_string()) {
        Some(s) => s,
        None => {
            return err(
                span,
                "account_id! expects a single string literal, e.g. account_id!(\"r...\")",
            );
        }
    };

    let bytes = match base58::decode(&address) {
        Ok(bytes) => bytes,
        Err(e) => return err(span, &e.message()),
    };

    let src = format!(
        "::hooks_lib::types::AccountId([{}])",
        bytes
            .iter()
            .map(|b| format!("0x{b:02X}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    src.parse::<TokenStream>()
        .unwrap_or_else(|_| err(span, "hooks-macros: internal account_id! expansion failed"))
}

/// Strips a `proc_macro::Literal`'s `to_string()` output down to a plain
/// Rust `String`, if it is a plain (unprefixed, unsuffixed) string literal —
/// i.e. exactly `"..."`, escape sequences included verbatim as written
/// (r-addresses never contain characters needing escaping, so no unescaping
/// is attempted; a `"..."` string with backslashes would round-trip through
/// [`base58::decode`] as literal backslash characters, which simply fail to
/// decode as base58 like any other invalid character). Returns `None` for
/// anything else (raw strings, byte strings, non-string literals, ...).
fn unquote_str(text: &str) -> Option<String> {
    let inner = text.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.to_string())
}

/// Shared implementation for [`hook`] and [`cbak`]: validates the annotated
/// item's shape, then appends a generated `extern "C"` export named
/// `export_name` that calls it.
fn entry_point(export_name: &str, attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return err(
            Span::call_site(),
            &format!("#[{export_name}] takes no arguments"),
        );
    }

    let tail = match extract_fn_name(item.clone(), export_name) {
        Ok(name) => match build_wrapper(export_name, &name) {
            Ok(wrapper) => wrapper,
            Err(e) => e,
        },
        Err(e) => e,
    };

    let mut out = item;
    out.extend(tail);
    out
}

/// Walks `item`'s tokens far enough to confirm it is a plain, no-argument,
/// `i64`-returning `fn`, and returns its name.
///
/// Deliberately tolerant of leading attributes (including doc comments) and
/// a leading `pub`/`pub(...)` visibility on the item, since those are
/// legal (if unusual) on a hook entry point and don't affect the generated
/// wrapper. Everything else about the shape is required exactly, since the
/// generated wrapper's `name()` call assumes it.
fn extract_fn_name(item: TokenStream, export_name: &str) -> Result<Ident, TokenStream> {
    let mut iter = item.into_iter().peekable();

    // Leading attributes: `#` followed by a `[...]` group, repeated.
    loop {
        match iter.peek() {
            Some(TokenTree::Punct(p)) if p.as_char() == '#' => {
                iter.next();
                match iter.next() {
                    Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Bracket => {}
                    Some(other) => {
                        return Err(err(other.span(), "malformed attribute before `fn`"));
                    }
                    None => return Err(err(Span::call_site(), "malformed attribute before `fn`")),
                }
            }
            _ => break,
        }
    }

    // Optional visibility: `pub` or `pub(...)`.
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

    // `fn` keyword: no `async`/`unsafe`/`const`/`extern` modifiers allowed.
    match iter.next() {
        Some(TokenTree::Ident(id)) if id.to_string() == "fn" => {}
        Some(other) => {
            return Err(err(
                other.span(),
                &format!(
                    "#[{export_name}] can only be applied to a plain `fn` item \
                     (no `async`/`unsafe`/`const`/`extern` modifiers)"
                ),
            ));
        }
        None => return Err(err(Span::call_site(), "expected a function")),
    }

    // Function name.
    let name = match iter.next() {
        Some(TokenTree::Ident(id)) => id,
        Some(other) => return Err(err(other.span(), "expected a function name")),
        None => return Err(err(Span::call_site(), "expected a function name")),
    };

    // No generics.
    if let Some(TokenTree::Punct(p)) = iter.peek() {
        if p.as_char() == '<' {
            return Err(err(
                p.span(),
                &format!("#[{export_name}] does not support generic functions"),
            ));
        }
    }

    // Argument list: must be present and empty.
    match iter.next() {
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis => {
            if !g.stream().is_empty() {
                return Err(err(
                    g.span(),
                    &format!("#[{export_name}] functions must take no arguments"),
                ));
            }
        }
        Some(other) => return Err(err(other.span(), "expected `()` after the function name")),
        None => return Err(err(name.span(), "expected `()` after the function name")),
    }

    // Return type: exactly `-> i64`.
    match iter.next() {
        Some(TokenTree::Punct(p)) if p.as_char() == '-' => {}
        Some(other) => {
            return Err(err(
                other.span(),
                &format!("#[{export_name}] functions must return `i64` (expected `-> i64`)"),
            ));
        }
        None => return Err(err(name.span(), "expected `-> i64`")),
    }
    match iter.next() {
        Some(TokenTree::Punct(p)) if p.as_char() == '>' => {}
        Some(other) => return Err(err(other.span(), "expected `-> i64`")),
        None => return Err(err(name.span(), "expected `-> i64`")),
    }
    match iter.next() {
        Some(TokenTree::Ident(id)) if id.to_string() == "i64" => {}
        Some(other) => {
            return Err(err(
                other.span(),
                &format!("#[{export_name}] functions must return `i64`"),
            ));
        }
        None => return Err(err(name.span(), "expected `i64` return type")),
    }

    // Body: a single `{ .. }` group, nothing after it (no `where` clause).
    match iter.next() {
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {}
        Some(other) => return Err(err(other.span(), "expected a function body")),
        None => return Err(err(name.span(), "expected a function body")),
    }
    if let Some(extra) = iter.next() {
        return Err(err(
            extra.span(),
            &format!(
                "#[{export_name}]: unexpected tokens after the function body \
                 (`where` clauses are not supported)"
            ),
        ));
    }

    Ok(name)
}

/// Builds the `extern "C"` export calling `target_name`, named
/// `export_name`.
fn build_wrapper(export_name: &str, target_name: &Ident) -> Result<TokenStream, TokenStream> {
    let src = format!(
        "#[unsafe(no_mangle)] pub extern \"C\" fn {export_name}(_reserved: u32) -> i64 {{ {target_name}() }}"
    );
    src.parse::<TokenStream>().map_err(|_| {
        err(
            Span::call_site(),
            "hooks-macros: internal wrapper generation failed",
        )
    })
}

/// Builds a `compile_error!("msg");` item at `span`, so validation failures
/// surface as a normal, well-located compile error rather than a macro
/// panic. `pub(crate)` (not private) so [`hook_data`]'s parser can share it.
pub(crate) fn err(span: Span, msg: &str) -> TokenStream {
    let mut args = TokenStream::new();
    args.extend([TokenTree::Literal(Literal::string(msg))]);
    let group = Group::new(Delimiter::Parenthesis, args);

    let mut out = TokenStream::new();
    out.extend([
        TokenTree::Ident(Ident::new("compile_error", span)),
        TokenTree::Punct(Punct::new('!', Spacing::Alone)),
        TokenTree::Group(group),
        TokenTree::Punct(Punct::new(';', Spacing::Alone)),
    ]);
    out
}

/// Identifier-concatenation macro backing `hooks_lib::txn_template!`'s
/// `set_<field>` setter names on stable Rust (replaces the nightly
/// `${concat(set_, $field)}` metavariable expression).
///
/// Scans its input for bracket groups shaped like `[< tok tok .. >]` (first
/// inner token `<`, last inner token `>`, both plain `Punct`s) and replaces
/// each with a single new identifier formed by concatenating the string
/// form of every `Ident` token strictly between them. Recurses into every
/// other group unchanged, so this can wrap an arbitrarily large token
/// stream (an entire `impl` block, in `txn_template!`'s case) and only the
/// marked splice points are touched.
///
/// Only ever invoked internally, from `txn_template!`'s own expansion
/// (`$crate::__paste! { .. }`) — not part of the public API.
#[doc(hidden)]
#[proc_macro]
pub fn paste(input: TokenStream) -> TokenStream {
    rewrite_stream(input)
}

/// Applies [`rewrite_tree`] to every token in `input`.
fn rewrite_stream(input: TokenStream) -> TokenStream {
    input.into_iter().map(rewrite_tree).collect()
}

/// Rewrites a single token: a `[< .. >]` splice marker becomes the
/// concatenated identifier; any other group is recursed into (with its
/// delimiter and span preserved); anything else passes through unchanged.
fn rewrite_tree(tt: TokenTree) -> TokenTree {
    match tt {
        TokenTree::Group(group) => {
            if group.delimiter() == Delimiter::Bracket {
                if let Some(ident) = try_concat_marker(group.stream()) {
                    return TokenTree::Ident(ident);
                }
            }
            let mut rewritten = Group::new(group.delimiter(), rewrite_stream(group.stream()));
            rewritten.set_span(group.span());
            TokenTree::Group(rewritten)
        }
        other => other,
    }
}

/// If `stream` is shaped exactly like a `< ident ident .. >` splice marker
/// (at least one `Ident` strictly between a leading and trailing `Punct`
/// token spelled `<`/`>`), returns the concatenated identifier. Returns
/// `None` for anything else (including a marker whose interior contains a
/// non-`Ident` token) — such a group is left as ordinary bracketed tokens,
/// which is never valid `hooks_lib` usage but is not this macro's problem
/// to diagnose.
///
/// Concatenating only `Ident` tokens (never arbitrary token text) is what
/// guarantees the result is itself always a valid identifier: every `Ident`
/// token's text already satisfies Rust's identifier grammar, and
/// concatenating any number of valid identifiers end-to-end yields another
/// valid identifier (an identifier's continuation characters are a superset
/// of its allowed starting characters). So `Ident::new` below can never
/// panic on the text this function builds.
fn try_concat_marker(stream: TokenStream) -> Option<Ident> {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    if tokens.len() < 3 {
        return None;
    }
    let first = tokens.first()?;
    let last = tokens.last()?;
    if !is_punct(first, '<') || !is_punct(last, '>') {
        return None;
    }
    let middle = tokens.get(1..tokens.len().saturating_sub(1))?;

    let mut text = String::new();
    for t in middle {
        match t {
            TokenTree::Ident(id) => text.push_str(&id.to_string()),
            _ => return None,
        }
    }
    if text.is_empty() {
        return None;
    }
    Some(Ident::new(&text, Span::call_site()))
}

/// Whether `tt` is a bare `Punct` token spelled `ch`.
fn is_punct(tt: &TokenTree, ch: char) -> bool {
    matches!(tt, TokenTree::Punct(p) if p.as_char() == ch)
}
