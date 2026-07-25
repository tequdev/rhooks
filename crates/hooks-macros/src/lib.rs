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
//!
//! # Why hand-rolled `proc_macro`, not `syn`/`quote`
//!
//! Both macros here only ever need to recognize a handful of token shapes
//! (a no-argument, `i64`-returning `fn` item; a `[< ident ident >]` splice
//! marker) — never a general Rust-item parser. This crate's own build
//! output is host tooling, not a wasm Hook artifact, so the byte-size
//! budget that governs `hooks-lib`/`hooks-core` doesn't apply here directly
//! — but it still governs indirectly, because `hooks-macros` is a
//! mandatory build-time dependency of *every* hook crate: `syn`+`quote`'s
//! (non-trivial, transitively-heavy) compile cost would be paid on every
//! `cargo build`/`cargo check` of every hook, for a token-shape-matching
//! job simple enough for direct `proc_macro::TokenStream` walking. A
//! std-only `proc_macro` crate with zero dependencies is the cheaper
//! choice given how small and stable those shapes are.

use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};

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
/// panic.
fn err(span: Span, msg: &str) -> TokenStream {
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
