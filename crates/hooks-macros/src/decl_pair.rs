//! Shared parser + codegen backing the `hook_state!`/`hook_parameter!`/
//! `otxn_parameter!` declaration-macro grammar — a "staircase" of four
//! forms (plus a backward-compatible fifth, pre-existing form), from a
//! fully-fixed key/name down to a fully composite, runtime-constructed one:
//!
//! ```text
//! // Form 1: fully fixed key/name (a zero-sized-type struct is declared).
//! hook_state!(RewardRateKey = b"RR" => XFL);
//!
//! // Form 2: a struct-shaped key with a fixed instance (a `const` of the
//! // struct's own name is declared alongside it).
//! hook_state!(CounterKey {name: [u8; 7]} = {name: b"counter"} => u64);
//!
//! // Form 3: a struct-shaped key, constructed at each call site.
//! hook_state!(DepositKey {tag: u8, owner: AccountId} => Deposit);
//!
//! // Form 4: a newtype (tuple struct) wrapping one existing type.
//! hook_state!(AccountKey AccountId => AccountData {balance: XFL, sequence: u16});
//!
//! // Existing form (backward-compatible): pairs two already-declared types.
//! hook_state!(SomeExistingKey => SomeExistingValue);
//! ```
//!
//! The value side (after `=>`) independently accepts either an
//! already-declared type name (as above) or an *inline* definition —
//! `=> Name { field: Type, .. }` — which generates a fresh
//! `#[derive(HookData)]`-equivalent (state role) or
//! `#[derive(ParamValue)]`-equivalent (parameter role) struct named `Name`.
//!
//! `hook_parameter!`/`otxn_parameter!` accept the identical grammar (Forms
//! 1–4 plus the existing two-type form), targeting
//! `hooks_lib::convert::TypedParamName` instead of
//! `hooks_lib::state::TypedStateKey`, **plus** one more backward-compatible
//! form neither of them shares with `hook_state!`: the original
//! *comma*-separated 3-argument form, `hook_parameter!($Name, $bytes =>
//! $Ty)`, where `$Name` is a marker type the caller already declared
//! separately (see [`LegacyFixed`](KeySpec::LegacyFixed)).
//!
//! # Why one shared module for three macros
//!
//! `hook_state!`'s key side and `hook_parameter!`/`otxn_parameter!`'s name
//! side play near-identical roles (see `hooks_lib::state::TypedStateKey`'s
//! doc comment's comparison table with `hooks_lib::convert::TypedParamName`)
//! — same four-form grammar staircase, same per-field codegen shape (reused
//! directly from [`crate::hook_key`]/[`crate::param_name`]), differing only
//! in which trait gets paired (`StateKeyEncode`+`TypedStateKey` vs. a
//! `1..=32`-bound-checked `TypedParamName`) and in one extra
//! backward-compatible form parameters alone still support. Parsing and
//! struct/field codegen live here, once; [`Role`] parameterizes the small
//! remaining difference at the two places it actually matters (the
//! key/name-side trait impl, and whether a comma after the name is legal).
//!
//! # Why hand-rolled, not `syn`/`quote`
//!
//! Same reasoning as every other macro in this crate (see the crate doc
//! comment): the grammar above is a handful of small, fixed token shapes,
//! not general Rust syntax — a single pass over a flat token buffer with
//! bounded lookahead (2–3 tokens) is enough to disambiguate every form,
//! with no need for a general expression/type parser. Field-list parsing
//! and per-field `ToBytes`/`FromBytes` codegen are reused verbatim from
//! [`crate::shape`]/[`crate::hook_key`]/[`crate::hook_data`]/
//! [`crate::param_name`]/[`crate::param_value`] — this module only adds the
//! *declaration* layer (parsing the grammar above into a struct
//! definition, plus the pairing impl) on top of codegen those modules
//! already provide for a struct's fields.

use crate::err;
use crate::shape::{FieldShape, StructShape, parse_fields, tokens_to_string};
use crate::{hook_data, hook_key, param_name, param_value};
use proc_macro::{Delimiter, Span, TokenStream, TokenTree};

/// Which pairing trait an invocation targets — the one place `hook_state!`
/// and `hook_parameter!`/`otxn_parameter!`'s otherwise-identical grammar and
/// codegen genuinely differ. See the module doc comment.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// `hook_state!` — pairs a key with `hooks_lib::state::TypedStateKey`.
    State,
    /// `hook_parameter!` — pairs a name with
    /// `hooks_lib::convert::TypedParamName`, read via `hook_param_typed`.
    HookParam,
    /// `otxn_parameter!` — identical to [`Role::HookParam`] except for the
    /// macro name in diagnostics (read via `otxn_param_typed`).
    OtxnParam,
}

impl Role {
    fn macro_name(self) -> &'static str {
        match self {
            Role::State => "hook_state!",
            Role::HookParam => "hook_parameter!",
            Role::OtxnParam => "otxn_parameter!",
        }
    }

    fn is_param(self) -> bool {
        matches!(self, Role::HookParam | Role::OtxnParam)
    }
}

/// The key/name half of a parsed declaration.
enum KeySpec {
    /// Backward-compatible: an already-declared type, used as the key/name
    /// as-is (`ExistingKey => ..`). Nothing is declared for it here.
    Existing { ty: String },
    /// `hook_parameter!`/`otxn_parameter!` only: the original comma-form,
    /// `$Name, $bytes => $Ty` — `$Name` is a marker type the caller already
    /// declared separately (e.g. `struct CfgName;`), so (unlike every other
    /// form below) nothing is declared for it, and its name is not
    /// naming-checked (this macro did not declare it).
    LegacyFixed { name: String, bytes: String },
    /// Form 1: `$Name = $bytes => ..` — declares `$Name` as a new
    /// zero-sized unit struct.
    Fixed {
        name: String,
        name_span: Span,
        bytes: String,
    },
    /// Forms 2/3: `$Name { field: Type, .. } [= { field: value, .. }] => ..`
    /// — declares `$Name` as a new named-field struct, optionally with a
    /// fixed instance (`init`, Form 2) declared as a `const` of the same
    /// name.
    Struct {
        name: String,
        name_span: Span,
        fields: Vec<FieldShape>,
        init: Option<String>,
    },
    /// Form 4: `$Name $Inner => ..` — declares `$Name` as a new tuple
    /// struct wrapping the single existing type `$Inner`.
    Newtype {
        name: String,
        name_span: Span,
        inner_ty: String,
    },
}

/// The value/`Ty` half of a parsed declaration.
enum ValueSpec {
    /// An already-declared type, used as the value/`Ty` as-is.
    Existing { ty: String },
    /// `=> $Name { field: Type, .. }` — declares `$Name` as a new
    /// named-field struct (a `#[derive(HookData)]`/`#[derive(ParamValue)]`
    /// equivalent, depending on [`Role`]) and uses it as the value.
    Inline {
        name: String,
        name_span: Span,
        fields: Vec<FieldShape>,
    },
}

/// A flat, randomly-indexable token buffer with bounded lookahead — every
/// form in this module's grammar is disambiguated by looking at most 2–3
/// tokens ahead of the current position, which a `Vec`-backed cursor makes
/// simpler to express than a `Peekable` iterator (which only looks one
/// token ahead without extra buffering of its own).
struct Cursor {
    tokens: Vec<TokenTree>,
    pos: usize,
}

impl Cursor {
    fn new(input: TokenStream) -> Self {
        Self {
            tokens: input.into_iter().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<&TokenTree> {
        self.tokens.get(self.pos)
    }

    fn peek_at(&self, offset: usize) -> Option<&TokenTree> {
        self.pos
            .checked_add(offset)
            .and_then(|i| self.tokens.get(i))
    }

    fn bump(&mut self) -> Option<TokenTree> {
        let tt = self.tokens.get(self.pos).cloned();
        if tt.is_some() {
            self.pos = self.pos.wrapping_add(1);
        }
        tt
    }

    fn is_empty(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    /// Span to anchor an "unexpected end of input" error at: the last
    /// token's span if there is one, otherwise the call site.
    fn end_span(&self) -> Span {
        self.tokens
            .last()
            .map_or_else(Span::call_site, TokenTree::span)
    }

    /// Span of the token at the current position, or [`Cursor::end_span`]
    /// if the cursor is already exhausted.
    fn here_span(&self) -> Span {
        self.peek().map_or_else(|| self.end_span(), TokenTree::span)
    }
}

/// Whether the token at `idx` is a bare `=` — i.e. a `Punct` spelled `=`
/// that is *not* the first half of a `=>` arrow (checked by peeking one
/// token further).
fn is_bare_eq(c: &Cursor, idx: usize) -> bool {
    is_punct(c.peek_at(idx), '=') && !is_punct(c.peek_at(idx.wrapping_add(1)), '>')
}

/// Whether the token at `idx` starts a `=>` arrow (a `=` `Punct` directly
/// followed by a `>` `Punct`).
fn is_arrow_at(c: &Cursor, idx: usize) -> bool {
    is_punct(c.peek_at(idx), '=') && is_punct(c.peek_at(idx.wrapping_add(1)), '>')
}

fn is_punct(tt: Option<&TokenTree>, ch: char) -> bool {
    matches!(tt, Some(TokenTree::Punct(p)) if p.as_char() == ch)
}

/// Consumes the `=>` arrow at the cursor's current position (already
/// confirmed present by the caller via [`is_arrow_at`]).
fn bump_arrow(c: &mut Cursor) {
    c.bump();
    c.bump();
}

/// Collects every remaining token up to (not including) a top-level `=>`,
/// reconstructing it as source text via [`tokens_to_string`]. Used for
/// every "existing type" token run this grammar accepts (a key/name/value
/// type this macro does not itself declare).
fn collect_until_arrow(c: &mut Cursor, mac: &str) -> Result<String, TokenStream> {
    let mut collected: Vec<TokenTree> = Vec::new();
    loop {
        if is_arrow_at(c, 0) {
            if collected.is_empty() {
                return Err(err(
                    c.here_span(),
                    &format!("{mac}: expected a type before `=>`"),
                ));
            }
            return Ok(tokens_to_string(&collected));
        }
        match c.bump() {
            Some(tt) => collected.push(tt),
            None => {
                return Err(err(
                    c.end_span(),
                    &format!("{mac}: expected `=>` after the key/name type"),
                ));
            }
        }
    }
}

/// Enforces `rustc`'s own `non_camel_case_types` shape on a type name this
/// macro is about to declare: the first character must be an uppercase
/// ASCII letter, and the name must not contain an underscore (which would
/// signal `snake_case`/`SCREAMING_SNAKE_CASE`, never valid `UpperCamelCase`).
/// Only applied to names this macro itself declares (`Existing`/
/// `LegacyFixed` reference a type the *caller* already named, which this
/// macro has no business re-validating).
fn check_upper_camel_case(name: &str, span: Span, mac: &str) -> Result<(), TokenStream> {
    let mut chars = name.chars();
    let first_ok = matches!(chars.next(), Some(c) if c.is_ascii_uppercase());
    let no_underscore = !name.contains('_');
    if first_ok && no_underscore {
        return Ok(());
    }
    Err(err(
        span,
        &format!(
            "{mac}: `{name}` is not UpperCamelCase — a type name declared by \
             {mac} must start with an uppercase ASCII letter and contain no \
             underscores (e.g. `RewardRateKey`, not `reward_rate_key` or \
             `Reward_Rate_Key`)"
        ),
    ))
}

/// Parses one `hook_state!`/`hook_parameter!`/`otxn_parameter!` invocation's
/// argument tokens into a key/name spec and a value spec.
fn parse(input: TokenStream, role: Role) -> Result<(KeySpec, ValueSpec), TokenStream> {
    let mac = role.macro_name();
    let mut c = Cursor::new(input);

    if c.is_empty() {
        return Err(err(
            Span::call_site(),
            &format!("{mac}: expected a key/name and a value, e.g. {mac}(Key => Value)"),
        ));
    }

    let key = parse_key(&mut c, role, mac)?;

    if !is_arrow_at(&c, 0) {
        return Err(err(
            c.here_span(),
            &format!("{mac}: expected `=>` followed by the value type"),
        ));
    }
    bump_arrow(&mut c);

    let value = parse_value(&mut c, mac)?;

    if !c.is_empty() {
        return Err(err(
            c.here_span(),
            &format!("{mac}: unexpected tokens after the value"),
        ));
    }

    Ok((key, value))
}

fn parse_key(c: &mut Cursor, role: Role, mac: &str) -> Result<KeySpec, TokenStream> {
    let name_id = match c.peek() {
        Some(TokenTree::Ident(id)) => id.clone(),
        _ => {
            // Not a bare identifier at all (e.g. `[u8; 7]`) — can only be
            // an existing type used as-is.
            return Ok(KeySpec::Existing {
                ty: collect_until_arrow(c, mac)?,
            });
        }
    };

    // `Name { ... }` — Forms 2/3 (struct-shaped key).
    if let Some(TokenTree::Group(g)) = c.peek_at(1) {
        if g.delimiter() == Delimiter::Brace {
            let name = name_id.to_string();
            let name_span = name_id.span();
            check_upper_camel_case(&name, name_span, mac)?;
            c.bump(); // Name
            let group = match c.bump() {
                Some(TokenTree::Group(g)) => g,
                _ => unreachable!("peeked a brace group above"),
            };
            let fields = parse_fields(group.stream(), mac)?;
            if fields.is_empty() {
                return Err(err(
                    name_span,
                    &format!("{mac}: `{name} {{ .. }}` must have at least one field"),
                ));
            }

            let init = if is_bare_eq(c, 0) {
                c.bump(); // `=`
                match c.bump() {
                    Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
                        let inits: Vec<TokenTree> = g.stream().into_iter().collect();
                        Some(tokens_to_string(&inits))
                    }
                    Some(other) => {
                        return Err(err(
                            other.span(),
                            &format!(
                                "{mac}: expected `{{ field: value, .. }}` after `=` \
                                 (a fixed instance of `{name}`)"
                            ),
                        ));
                    }
                    None => {
                        return Err(err(
                            c.end_span(),
                            &format!(
                                "{mac}: expected `{{ field: value, .. }}` after `=` \
                                 (a fixed instance of `{name}`)"
                            ),
                        ));
                    }
                }
            } else {
                None
            };

            return Ok(KeySpec::Struct {
                name,
                name_span,
                fields,
                init,
            });
        }
    }

    // `Name = bytes => ..` — Form 1 (fixed-byte-string ZST key/name).
    if is_bare_eq(c, 1) {
        let name = name_id.to_string();
        let name_span = name_id.span();
        check_upper_camel_case(&name, name_span, mac)?;
        c.bump(); // Name
        c.bump(); // `=`
        let bytes = collect_until_arrow(c, mac)?;
        return Ok(KeySpec::Fixed {
            name,
            name_span,
            bytes,
        });
    }

    // `Name, bytes => Ty` — parameters' original comma-form only.
    if is_punct(c.peek_at(1), ',') {
        if !role.is_param() {
            return Err(err(
                c.peek_at(1).map_or_else(Span::call_site, TokenTree::span),
                &format!(
                    "{mac}: unexpected `,` after `{name_id}` — did you mean `=`? \
                     (e.g. `{mac}({name_id} = b\"...\" => Ty)`)"
                ),
            ));
        }
        let name = name_id.to_string();
        c.bump(); // Name — not naming-checked: not declared by this macro.
        c.bump(); // `,`
        let bytes = collect_until_arrow(c, mac)?;
        return Ok(KeySpec::LegacyFixed { name, bytes });
    }

    // `Name => ..` (the arrow starts immediately) — the existing,
    // backward-compatible two-type form, `Name` a bare already-declared
    // type.
    if is_arrow_at(c, 1) {
        c.bump(); // Name
        return Ok(KeySpec::Existing {
            ty: name_id.to_string(),
        });
    }

    // Anything immediately after the leading identifier that continues the
    // *same* type — `::` (a multi-segment path, e.g. `crate::MyKey`) or `<`
    // (generic args) — means this whole run is one existing, more complex
    // type (the backward-compatible form): collect it verbatim.
    if is_punct(c.peek_at(1), ':') || is_punct(c.peek_at(1), '<') {
        return Ok(KeySpec::Existing {
            ty: collect_until_arrow(c, mac)?,
        });
    }

    // Anything else immediately after the leading identifier — another
    // bare identifier, a `[..]` array type, a `&` reference, ... — starts
    // a *second*, independent type, with nothing joining it to `Name`. No
    // valid Rust `ty` is ever spelled as two space-separated type
    // expressions with no connecting token, so this is unambiguous: Form 4
    // (newtype), `Name` wrapping whatever type follows.
    let name = name_id.to_string();
    let name_span = name_id.span();
    check_upper_camel_case(&name, name_span, mac)?;
    c.bump(); // Name
    let inner_ty = collect_until_arrow(c, mac)?;
    Ok(KeySpec::Newtype {
        name,
        name_span,
        inner_ty,
    })
}

fn parse_value(c: &mut Cursor, mac: &str) -> Result<ValueSpec, TokenStream> {
    if let Some(TokenTree::Ident(name_id)) = c.peek() {
        if let Some(TokenTree::Group(g)) = c.peek_at(1) {
            if g.delimiter() == Delimiter::Brace {
                let name = name_id.to_string();
                let name_span = name_id.span();
                check_upper_camel_case(&name, name_span, mac)?;
                c.bump(); // Name
                let group = match c.bump() {
                    Some(TokenTree::Group(g)) => g,
                    _ => unreachable!("peeked a brace group above"),
                };
                let fields = parse_fields(group.stream(), mac)?;
                if fields.is_empty() {
                    return Err(err(
                        name_span,
                        &format!("{mac}: `{name} {{ .. }}` must have at least one field"),
                    ));
                }
                if !c.is_empty() {
                    return Err(err(
                        c.here_span(),
                        &format!("{mac}: unexpected tokens after the inline value definition"),
                    ));
                }
                return Ok(ValueSpec::Inline {
                    name,
                    name_span,
                    fields,
                });
            }
        }
    }

    if c.is_empty() {
        return Err(err(
            c.end_span(),
            &format!("{mac}: expected a value type after `=>`"),
        ));
    }
    let mut rest: Vec<TokenTree> = Vec::new();
    while let Some(tt) = c.bump() {
        rest.push(tt);
    }
    Ok(ValueSpec::Existing {
        ty: tokens_to_string(&rest),
    })
}

/// Renders a plain `struct Name;` (unit struct) or `struct Name(Inner);`
/// (tuple struct, `Inner` supplied verbatim) declaration.
fn tuple_or_unit_struct_decl(name: &str, inner_ty: Option<&str>) -> String {
    match inner_ty {
        Some(ty) => format!("struct {name}({ty});\n"),
        None => format!("struct {name};\n"),
    }
}

/// Renders a plain named-field `struct Name { field: Type, .. }`
/// declaration from an already-parsed field list.
fn named_struct_decl(name: &str, fields: &[FieldShape]) -> String {
    let mut body = String::new();
    for f in fields {
        body.push_str(&format!("{field}: {ty},\n", field = f.name, ty = f.ty));
    }
    format!("struct {name} {{\n{body}}}\n")
}

/// Generates the key/name-side `ToBytes` + pairing-trait-supertrait impls
/// for a field-based struct (Forms 2/3/4) — [`hook_key::generate`] for
/// [`Role::State`], [`param_name::generate`] for a parameter role.
fn key_struct_codegen(shape: &StructShape, role: Role) -> String {
    match role {
        Role::State => hook_key::generate(shape).to_string(),
        Role::HookParam | Role::OtxnParam => param_name::generate(shape).to_string(),
    }
}

/// Generates the value-side `FromBytes`/`FixedRead` (+, for state,
/// `ToBytes`/`LEN`) impls for an inline value struct —
/// [`hook_data::generate`] for [`Role::State`], [`param_value::generate`]
/// for a parameter role.
fn value_struct_codegen(shape: &StructShape, role: Role) -> String {
    match role {
        Role::State => hook_data::generate(shape).to_string(),
        Role::HookParam | Role::OtxnParam => param_value::generate(shape).to_string(),
    }
}

/// Generates the ordinary pairing impl — `impl TypedStateKey for {key} {
/// type Value = {value}; }` (state) or `impl TypedParamName for {key} {
/// type Value = {value}; ..}` (parameters) — this is the form every
/// backward-compatible/Form-2/3/4 declaration uses.
///
/// For the parameter case, also overrides
/// [`TypedParamName::with_name_bytes`](::hooks_lib::convert::TypedParamName::with_name_bytes)
/// to allocate exactly `<{key} as ToBytes>::MAX_LEN` bytes instead of
/// relying on the trait's generic default (a full
/// [`PARAM_NAME_MAX_LEN`](::hooks_lib::convert::PARAM_NAME_MAX_LEN)
/// scratch buffer — see that method's doc comment for why the default
/// can't size itself this precisely). This works here because the
/// allocation sits inside a concrete, non-generic `impl` block — `{key}`
/// is a real type at codegen time, not a generic parameter — so
/// `MAX_LEN` is an ordinary compile-time constant.
fn pairing_impl(key: &str, value: &str, role: Role) -> String {
    match role {
        Role::State => format!(
            "
#[automatically_derived]
impl ::hooks_lib::state::TypedStateKey for {key} {{
    type Value = {value};
}}
"
        ),
        Role::HookParam | Role::OtxnParam => format!(
            "
#[automatically_derived]
impl ::hooks_lib::convert::TypedParamName for {key} {{
    type Value = {value};

    #[inline(always)]
    fn with_name_bytes<__R>(&self, f: impl ::core::ops::FnOnce(&[u8]) -> __R) -> __R {{
        let mut __buf = [0u8; <{key} as ::hooks_lib::convert::ToBytes>::MAX_LEN];
        let _ = ::hooks_lib::convert::ToBytes::write(self, &mut __buf);
        f(&__buf)
    }}
}}
"
        ),
    }
}

/// Generates the fixed-byte-string `ToBytes` impl a Form-1/legacy-fixed key
/// or name shares: `MAX_LEN = { $bytes.len() }`, `write` delegating
/// straight to `$bytes`'s own `ToBytes::write` — the wire encoding *is*
/// the in-memory representation, nothing to compute (see
/// `hooks_lib::convert::TypedParamName`'s doc comment, "Zero-cost for
/// the plain-byte-string case").
fn fixed_bytes_to_bytes_impl(name: &str, bytes: &str) -> String {
    format!(
        "
#[automatically_derived]
impl ::hooks_lib::convert::ToBytes for {name} {{
    const MAX_LEN: usize = {{ ({bytes}).len() }};

    #[inline(always)]
    fn write(&self, buf: &mut [u8]) -> usize {{
        ::hooks_lib::convert::ToBytes::write({bytes}, buf)
    }}
}}
"
    )
}

/// Generates the parameter-role pairing for a fixed-byte-string name (Form
/// 1 or the legacy comma-form): overrides
/// [`TypedParamName::with_name_bytes`](::hooks_lib::convert::TypedParamName::with_name_bytes)
/// to hand `f` `$bytes` directly — a `'static` reference, no copy, no
/// buffer, no per-call encode — the zero-copy fast path
/// `hooks_lib::convert::TypedParamName`'s doc comment describes.
fn fixed_bytes_param_pairing(name: &str, bytes: &str, value: &str) -> String {
    format!(
        "
#[automatically_derived]
impl ::hooks_lib::convert::TypedParamName for {name} {{
    type Value = {value};

    #[inline(always)]
    fn with_name_bytes<__R>(&self, f: impl ::core::ops::FnOnce(&[u8]) -> __R) -> __R {{
        f({bytes})
    }}
}}
"
    )
}

/// Expands a parsed `(key, value)` pair into the full declaration —
/// struct(s), field-based codegen, and the pairing impl.
///
/// The value side is resolved *first* (it never depends on the key), so
/// that when the key side is processed, `value_ty` is already available —
/// letting [`KeySpec::Fixed`]/[`KeySpec::LegacyFixed`] (the two forms whose
/// pairing impl embeds the value type directly, rather than deferring to
/// the shared [`pairing_impl`]) finish in one pass with no placeholder
/// bookkeeping.
fn generate(key: KeySpec, value: ValueSpec, role: Role) -> TokenStream {
    let mut src = String::new();
    // Anchors the "internal codegen failed to parse" fallback below at a
    // real token from the invocation, updated as each declared name is
    // seen, rather than always pointing at the macro call site.
    let mut err_span = Span::call_site();

    let value_ty = match value {
        ValueSpec::Existing { ty } => ty,
        ValueSpec::Inline {
            name,
            name_span,
            fields,
        } => {
            err_span = name_span;
            let shape = StructShape {
                name: name.clone(),
                name_span,
                fields,
            };
            src.push_str(&named_struct_decl(&name, &shape.fields));
            src.push_str(&value_struct_codegen(&shape, role));
            name
        }
    };

    // Forms whose pairing impl is uniform (`impl .. { type Value = ..; }`,
    // relying on the trait's default body) are collected here and finished
    // with `pairing_impl` at the end; `Fixed`/`LegacyFixed` instead finish
    // their own (non-uniform) pairing impl immediately, since state and
    // parameters need genuinely different bodies for them (see
    // `fixed_bytes_param_pairing`'s doc comment).
    let mut key_ty: Option<String> = None;

    match key {
        KeySpec::Existing { ty } => key_ty = Some(ty),
        KeySpec::LegacyFixed { name, bytes } => {
            // Parameters' original comma-form: `Name` is declared
            // *elsewhere* by the caller — only the impls are generated.
            src.push_str(&fixed_bytes_to_bytes_impl(&name, &bytes));
            src.push_str(&param_name::param_name_length_assert(&name));
            src.push_str(&fixed_bytes_param_pairing(&name, &bytes, &value_ty));
        }
        KeySpec::Fixed {
            name,
            name_span,
            bytes,
        } => {
            err_span = name_span;
            src.push_str(&tuple_or_unit_struct_decl(&name, None));
            src.push_str(&fixed_bytes_to_bytes_impl(&name, &bytes));
            match role {
                Role::State => {
                    src.push_str(&hook_key::state_key_encode_impl(&name));
                    src.push_str(&pairing_impl(&name, &value_ty, role));
                }
                Role::HookParam | Role::OtxnParam => {
                    src.push_str(&param_name::param_name_length_assert(&name));
                    src.push_str(&fixed_bytes_param_pairing(&name, &bytes, &value_ty));
                }
            }
        }
        KeySpec::Struct {
            name,
            name_span,
            fields,
            init,
        } => {
            err_span = name_span;
            let shape = StructShape {
                name: name.clone(),
                name_span,
                fields,
            };
            src.push_str(&named_struct_decl(&name, &shape.fields));
            src.push_str(&key_struct_codegen(&shape, role));
            if let Some(init) = init {
                src.push_str(&format!(
                    "#[allow(non_upper_case_globals)]\nconst {name}: {name} = {name} {{ {init} }};\n"
                ));
            }
            key_ty = Some(name);
        }
        KeySpec::Newtype {
            name,
            name_span,
            inner_ty,
        } => {
            err_span = name_span;
            src.push_str(&tuple_or_unit_struct_decl(&name, Some(&inner_ty)));
            let shape = StructShape {
                name: name.clone(),
                name_span,
                fields: vec![FieldShape {
                    name: "0".to_string(),
                    ty: inner_ty,
                }],
            };
            src.push_str(&key_struct_codegen(&shape, role));
            key_ty = Some(name);
        }
    }

    if let Some(key_ty) = key_ty {
        src.push_str(&pairing_impl(&key_ty, &value_ty, role));
    }

    src.parse::<TokenStream>().unwrap_or_else(|_| {
        err(
            err_span,
            &format!(
                "hooks-macros: internal {} codegen failed to parse",
                role.macro_name()
            ),
        )
    })
}

/// Entry point invoked by `hook_state!`/`hook_parameter!`/`otxn_parameter!`
/// in `lib.rs` (one thin `#[proc_macro]` wrapper per macro name, `role`
/// pinning which one), mirroring the `#[proc_macro_derive]`/`derive(..)`
/// split the four struct derives already use.
pub(crate) fn expand(input: TokenStream, role: Role) -> TokenStream {
    match parse(input, role) {
        Ok((key, value)) => generate(key, value, role),
        Err(e) => e,
    }
}
