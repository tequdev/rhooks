//! Shared parser + codegen backing the `hook_state!`/`hook_parameter!`/
//! `otxn_parameter!` declaration-macro grammar.
//!
//! Every invocation names an **entity** first — the thing the hook actually
//! operates on ("this account's deposit record", "the `INS` parameter") —
//! followed by a declaration of the key/name that addresses it and the value
//! it holds:
//!
//! ```text
//! // Form 1: fully fixed key/name (a zero-sized-type key is declared).
//! hook_state!(RewardRate, RewardRateKey = b"RR" => XFL);
//!
//! // Form 2: a struct-shaped key with a fixed instance (a `const` of the
//! // entity's own name is declared alongside it).
//! hook_state!(Counter, CounterKey {name: [u8; 7]} = {name: *b"counter"} => u64);
//!
//! // Form 3: a struct-shaped key, constructed at each call site.
//! hook_state!(DepositState, DepositKey {tag: u8, owner: AccountId} => Deposit);
//!
//! // Form 4: a newtype (tuple struct) wrapping one existing type.
//! hook_state!(AccountState, AccountKey AccountId => AccountData {balance: XFL});
//!
//! // `existing` keyword form: key/name impls attached to a type the caller
//! // declared itself (so it can carry its own visibility, derives, docs).
//! struct MyOwnKey;
//! hook_state!(MyOwnState, existing MyOwnKey = b"MK" => u64);
//!
//! // Pairing form: wraps a key type the caller already declared.
//! hook_state!(CounterState, SomeExistingKey => SomeExistingValue);
//! ```
//!
//! # The entity is the primary surface
//!
//! The entity type carries **all** the inherent accessors —
//! `get_state`/`set_state`/`update_state`/`delete_state` for hook state,
//! `get_value` (plus `get_name` on the fixed-byte-string forms) for a
//! parameter — and implements the role traits itself, so it is equally usable
//! with the free, loose and `_foreign` APIs
//! (`state_foreign_get_typed(&entity, ..)` and friends all work):
//!
//! ```text
//! hook_state!(DepositState, DepositKey {tag: u8, owner: AccountId} => Deposit {amount: u64});
//!
//! let deposit = DepositState { tag: 1, owner };   // the entity's own fields
//! let current = deposit.get_state()?;
//! deposit.set_state(&Deposit { amount: 1 })?;
//! deposit.delete_state()?;
//! ```
//!
//! **No generated code ever constructs a key/name type.** The entity encodes
//! *itself*: a fixed form encodes its literal, a struct form encodes its own
//! mirrored fields through the same per-field codegen the key side uses, and
//! the pairing form forwards through `&self.0`. That is what lets the
//! `existing` and pairing forms work with caller-owned types that may be
//! non-`Copy`, privately constructed, or impossible to build at all from
//! here.
//!
//! The key/name type stays declared — the identifier component deserves a
//! name — as a **trait carrier without inherent accessors**, exactly usable
//! with the free/loose/foreign APIs, and never touched by generated code.
//!
//! The value side (after `=>`) independently accepts either an
//! already-declared type name (as above) or an *inline* definition —
//! `=> Name { field: Type, .. }` — which generates a fresh
//! `#[derive(HookData)]`-equivalent (state role) or
//! `#[derive(ParamValue)]`-equivalent (parameter role) struct named `Name`.
//!
//! An optional leading visibility token (`pub`, `pub(crate)`, ...) applies to
//! **every** item the invocation declares — all-or-nothing, defaulting to
//! private. See [`Vis`] for the precondition that carries.
//!
//! `hook_parameter!`/`otxn_parameter!` accept the identical grammar,
//! targeting `rshooks::convert::TypedParamName` instead of
//! `rshooks::state::TypedStateKey`.
//!
//! # Why one shared module for three macros
//!
//! `hook_state!`'s key side and `hook_parameter!`/`otxn_parameter!`'s name
//! side play near-identical roles (see `rshooks::state::TypedStateKey`'s
//! doc comment's comparison table with `rshooks::convert::TypedParamName`)
//! — same grammar staircase, same per-field codegen shape (reused directly
//! from [`crate::hook_key`]/[`crate::param_name`]), differing only in which
//! trait gets paired (`StateKeyEncode`+`TypedStateKey` vs. a
//! `1..=32`-bound-checked `TypedParamName`) and in which inherent accessors
//! the entity gets (a state entry can be read, written, updated and deleted;
//! a parameter is read-only from the reading hook's perspective).
//! Parsing and struct/field codegen live here, once; [`Role`] parameterizes
//! the small remaining difference at the two places it actually matters
//! (the key/name-side trait impls, and the accessor block).
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
//! *declaration* layer (parsing the grammar above into struct definitions,
//! plus the pairing and accessor impls) on top of codegen those modules
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
    /// `hook_state!` — pairs a key with `rshooks::state::TypedStateKey`.
    State,
    /// `hook_parameter!` — pairs a name with
    /// `rshooks::convert::TypedParamName`, read via `hook_param_typed`.
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

    /// What this role's declarations are *about*, for the generated doc
    /// comments — "hook-state entry" vs "Hook API parameter".
    fn subject(self) -> &'static str {
        match self {
            Role::State => "hook-state entry",
            Role::HookParam | Role::OtxnParam => "Hook API parameter",
        }
    }

    /// What this role calls the addressing component, for the same.
    fn component(self) -> &'static str {
        match self {
            Role::State => "key",
            Role::HookParam | Role::OtxnParam => "name",
        }
    }

    /// One accessor an entity of this role carries, for diagnostics that
    /// illustrate what an entity is *for*. `get_state` means nothing on a
    /// parameter entity, which is read-only.
    fn example_method(self) -> &'static str {
        match self {
            Role::State => "get_state",
            Role::HookParam | Role::OtxnParam => "get_value",
        }
    }
}

/// The optional leading visibility token, applied verbatim to every item the
/// invocation declares (entity, its fields, key/name struct and its fields,
/// an inline value struct and its fields, a Form 2 `const`).
///
/// All-or-nothing on purpose: a declaration is one conceptual unit, and a
/// `pub` entity whose value type is private is not a usable API — rustc would
/// reject it anyway (see below), so there is no combination worth spelling
/// separately. Absent, everything stays private (module-local), which is what
/// a hook's own state declarations almost always want.
///
/// **Precondition (documented, rustc-enforced).** Making the *generated*
/// items public does not make a *caller-owned* type public. Every
/// caller-owned type that ends up in a public generated field or associated
/// type — a pairing `K`, an already-declared value `V`, a Form 4 `Inner`, any
/// field type — must be at least as visible as the invocation's visibility,
/// or rustc reports its own `E0446`/`E0445` ("private type in public
/// interface") at the expansion. The one exception is an `existing` form's
/// `N`: it never appears in the entity's public fields or associated types,
/// so a private `N` under a `pub` invocation is perfectly valid.
struct Vis {
    /// The visibility as source text (`""`, `"pub"`, `"pub(crate)"`, ...),
    /// ready to splice ahead of an item or a field.
    text: String,
}

/// The mandatory leading **entity** name — the type this invocation's
/// accessors hang off, and the primary thing a hook interacts with.
struct Entity {
    /// The entity's name, verbatim.
    ///
    /// No span is kept: every diagnostic that mentions the entity anchors at
    /// the *other* token — the colliding key/name or value — which is both
    /// the second occurrence the caller has to change and the one the naming
    /// checks already have a span for.
    name: String,
}

/// The key/name half of a parsed declaration.
enum KeySpec {
    /// Pairing form, `$Entity, $K => $V` — `$K` is a key/name type the caller
    /// already declared *and* already gave the underlying encoding trait to
    /// (a `#[derive(HookKey)]` struct, a `state_keys!` enum, a
    /// `#[derive(ParamName)]` struct). Nothing is declared for it; the entity
    /// becomes a newtype wrapper forwarding through `&self.0`.
    Pairing { ty: String, ty_span: Span },
    /// The `existing` keyword form, `$Entity, existing $N = $bytes => $V` —
    /// `$N` is a key/name type the caller declared separately (e.g.
    /// `pub struct CfgName;`, carrying its own visibility/derives/docs), so
    /// no struct is declared for it, and its name is not naming-checked (this
    /// macro did not declare it). It receives the same trait impls Form 1
    /// generates for its key type — that is this form's whole purpose — while
    /// the entity encodes the literal directly, never constructing `$N`.
    ExistingFixed {
        name: String,
        name_span: Span,
        bytes: String,
    },
    /// Form 1: `$Entity, $N = $bytes => $V` — declares `$N` as a new
    /// zero-sized unit struct.
    Fixed {
        name: String,
        name_span: Span,
        bytes: String,
    },
    /// Forms 2/3: `$Entity, $K { field: Type, .. } [= { field: value, .. }]
    /// => $V` — declares `$K` as a new named-field struct, and mirrors its
    /// fields onto the entity. With an initializer (Form 2), a `const` of the
    /// **entity's** name holds the one fixed instance.
    Struct {
        name: String,
        name_span: Span,
        fields: Vec<FieldShape>,
        init: Option<String>,
    },
    /// Form 4: `$Entity, $K $Inner => $V` — declares `$K` as a new tuple
    /// struct wrapping the single existing type `$Inner`, and mirrors that
    /// shape onto the entity.
    Newtype {
        name: String,
        name_span: Span,
        inner_ty: String,
    },
}

impl KeySpec {
    /// The declared key/name identifier and its span, for the collision
    /// checks — `None` for the forms whose key/name this macro does not
    /// declare (their spelling is the caller's business).
    fn declared_ident(&self) -> Option<(&str, Span)> {
        match self {
            KeySpec::Fixed {
                name, name_span, ..
            }
            | KeySpec::Struct {
                name, name_span, ..
            }
            | KeySpec::Newtype {
                name, name_span, ..
            } => Some((name, *name_span)),
            KeySpec::Pairing { .. } | KeySpec::ExistingFixed { .. } => None,
        }
    }

    /// The key/name identifier as written, declared here or not — used by the
    /// entity-vs-key collision check, which must fire even when the macro
    /// only *references* the type (a caller cannot name their entity and
    /// their `existing` marker the same thing either).
    fn ident_text(&self) -> Option<(&str, Span)> {
        match self {
            KeySpec::ExistingFixed {
                name, name_span, ..
            } => Some((name, *name_span)),
            KeySpec::Pairing { ty, ty_span } => Some((ty, *ty_span)),
            _ => self.declared_ident(),
        }
    }
}

/// The value/`Ty` half of a parsed declaration.
enum ValueSpec {
    /// An already-declared type, used as the value/`Ty` as-is.
    Existing { ty: String, ty_span: Span },
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
/// every token run this grammar takes verbatim — a key/name type the macro
/// does not declare, a newtype's wrapped type, or a fixed form's name bytes.
///
/// `subject` names *what* was being collected, so a run that comes back
/// empty or never reaches its `=>` is reported in the caller's own terms
/// ("expected the fixed key/name bytes before `=>`") instead of a generic
/// "expected a type" that misdescribes half the call sites.
fn collect_until_arrow(c: &mut Cursor, mac: &str, subject: &str) -> Result<String, TokenStream> {
    let mut collected: Vec<TokenTree> = Vec::new();
    loop {
        if is_arrow_at(c, 0) {
            if collected.is_empty() {
                return Err(err(
                    c.here_span(),
                    &format!("{mac}: expected {subject} before `=>`"),
                ));
            }
            return Ok(tokens_to_string(&collected));
        }
        match c.bump() {
            Some(tt) => collected.push(tt),
            None => {
                return Err(err(
                    c.end_span(),
                    &format!("{mac}: expected `=>` after {subject}"),
                ));
            }
        }
    }
}

/// Enforces `rustc`'s own `non_camel_case_types` shape on a type name this
/// macro is about to declare: the first character must be an uppercase
/// ASCII letter, and the name must not contain an underscore (which would
/// signal `snake_case`/`SCREAMING_SNAKE_CASE`, never valid `UpperCamelCase`).
/// Applied to the entity and to every name this macro itself declares
/// ([`KeySpec::Pairing`]/[`KeySpec::ExistingFixed`] reference a type the
/// *caller* already named, which this macro has no business re-validating —
/// a caller's own marker type may perfectly well be spelled `snake_case` or
/// as a raw identifier).
fn check_upper_camel_case(name: &str, span: Span, mac: &str) -> Result<(), TokenStream> {
    // `Self` is UpperCamelCase and still cannot name a declared type: it is
    // a keyword, resolving to whatever `impl` block encloses it. Caught here
    // so the caller gets a naming diagnostic rather than the
    // "internal codegen failed to parse" fallback a `struct Self;` produces.
    if name == "Self" {
        return Err(err(
            span,
            &format!(
                "{mac}: `Self` is a Rust keyword and cannot name a declared \
                 type — pick a concrete name (e.g. `RewardRateKey`)"
            ),
        ));
    }
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

/// The identifier `name` actually denotes, with a raw-identifier escape
/// stripped: `r#Deposit` and `Deposit` are the *same* identifier, so the
/// collision checks have to compare them as one. Written as an escape only
/// because the caller needed to get a keyword past the lexer — which is why
/// the escape is not part of the name.
fn canonical_ident(name: &str) -> &str {
    name.strip_prefix("r#").unwrap_or(name)
}

/// Whether `name` is a plain `snake_case` identifier — the shape the removed
/// instance binder used, and the one leading-identifier spelling that gets a
/// migration diagnostic instead of the generic "expected an entity name".
fn is_snake_case(name: &str) -> bool {
    matches!(name.chars().next(), Some(ch) if ch.is_ascii_lowercase())
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

/// Parses the optional leading visibility token.
///
/// Recognized shapes are exactly Rust's: `pub`, and `pub(..)` with any
/// restriction group (`pub(crate)`, `pub(super)`, `pub(in path)`), which is
/// consumed verbatim rather than inspected — this macro has no reason to
/// understand a visibility, only to pass it through.
fn parse_vis(c: &mut Cursor) -> Vis {
    let is_pub = matches!(c.peek(), Some(TokenTree::Ident(id)) if id.to_string() == "pub");
    if !is_pub {
        return Vis {
            text: String::new(),
        };
    }
    let mut text = String::from("pub");
    c.bump(); // `pub`
    if let Some(TokenTree::Group(g)) = c.peek() {
        if g.delimiter() == Delimiter::Parenthesis {
            text.push_str(&g.to_string());
            c.bump(); // `(..)`
        }
    }
    Vis { text }
}

/// Parses the mandatory leading `$Entity ,`.
///
/// Every rejection routes to the most specific diagnostic available:
///
/// 1. `existing ,` — the `existing` keyword landed in entity position, which
///    almost certainly means the entity name was forgotten.
/// 2. A `snake_case` identifier + `,` — the shape of the **removed** instance
///    binder, so the diagnostic carries both migration recipes rather than
///    just naming the rule.
/// 3. An identifier + `,` that is not `UpperCamelCase` — the ordinary
///    declared-type naming check ([`check_upper_camel_case`]).
/// 4. Anything else — the entity is missing entirely.
fn parse_entity(c: &mut Cursor, role: Role) -> Result<Entity, TokenStream> {
    let mac = role.macro_name();
    let id = match c.peek() {
        Some(TokenTree::Ident(id)) if is_punct(c.peek_at(1), ',') => id.clone(),
        _ => {
            return Err(err(
                c.here_span(),
                &format!(
                    "{mac}: expected an entity name first: `{mac}(MyThing, ..)` \
                     — the entity is the type the generated accessors hang off \
                     (`MyThing.{method}()`), and every form declares one",
                    method = role.example_method(),
                ),
            ));
        }
    };
    let span = id.span();
    let name = id.to_string();

    if name == "existing" {
        return Err(err(
            span,
            &format!(
                "{mac}: `existing` is a declaration keyword, not an entity name \
                 — write `{mac}(MyThing, existing YourType = b\"..\" => Ty)`, \
                 naming the entity first"
            ),
        ));
    }
    if is_snake_case(&name) {
        return Err(err(span, &removed_binder(mac, &name)));
    }
    check_upper_camel_case(&name, span, mac)?;

    c.bump(); // Entity
    c.bump(); // `,`
    Ok(Entity { name })
}

/// The diagnostic for a leading `snake_case` identifier — the shape of the
/// instance binder this grammar used to accept.
///
/// It carries **both** migration recipes, because the binder's two accepted
/// forms migrate differently: a fixed binder becomes an entity plus an
/// ordinary `let`, while a struct binder's runtime initializer has no
/// successor at all (an `= { .. }` after a struct declaration is now
/// unambiguously Form 2's `const` initializer, a const-context expression) —
/// that case becomes an entity struct literal at the call site.
fn removed_binder(mac: &str, name: &str) -> String {
    format!(
        "{mac}: `{name}` is not an entity name — a leading snake_case \
         identifier was the instance binder, which no longer exists. For a \
         fixed key/name, declare `{mac}(Entity, Name = b\"..\" => Ty)` and \
         bind it with `let {name} = Entity;`. For a struct-shaped key, \
         declare `{mac}(Entity, Key {{ .. }} => Ty)` and construct the \
         entity where you use it: `let mut {name} = Entity {{ field: value \
         }};`"
    )
}

/// Rejects an invocation that spells two of its three identifiers the same
/// way — the entity, the key/name, and the value all become distinct items,
/// so a repeat is a duplicate definition (or a silently self-referential
/// pairing) that rustc would report from inside the expansion.
///
/// Only **bare identifiers** are compared. A complex path or generic type
/// (`crate::MyKey`, `Wrapper<u8>`) is left to rustc: this check exists to
/// turn the common typo into a diagnostic at the caller's own tokens, not to
/// re-implement type equality.
fn check_distinct(
    entity: &Entity,
    key: &KeySpec,
    value: &ValueSpec,
    mac: &str,
) -> Result<(), TokenStream> {
    let value_ident: Option<(&str, Span)> = match value {
        ValueSpec::Inline {
            name, name_span, ..
        } => Some((name, *name_span)),
        ValueSpec::Existing { ty, ty_span } => {
            // Bare identifiers only — a path or a generic type is left to
            // rustc. `r#` counts as bare: it escapes an identifier, it does
            // not make one complex.
            let bare =
                !canonical_ident(ty).contains(|ch: char| !(ch.is_alphanumeric() || ch == '_'));
            bare.then_some((ty.as_str(), *ty_span))
        }
    };

    if let Some((key_name, key_span)) = key.ident_text() {
        if canonical_ident(key_name) == canonical_ident(&entity.name) {
            return Err(err(
                key_span,
                &format!(
                    "{mac}: the entity and the key/name are both called \
                     `{key_name}` — they are two different types (the entity \
                     carries the accessors, the key/name carries the \
                     encoding), so they need two different names"
                ),
            ));
        }
    }
    if let Some((value_name, value_span)) = value_ident {
        if canonical_ident(value_name) == canonical_ident(&entity.name) {
            return Err(err(
                value_span,
                &format!(
                    "{mac}: the entity and the value are both called \
                     `{value_name}` — they are two different types, so they \
                     need two different names"
                ),
            ));
        }
        if let Some((key_name, _)) = key.declared_ident() {
            if canonical_ident(key_name) == canonical_ident(value_name) {
                return Err(err(
                    value_span,
                    &format!(
                        "{mac}: the key/name and the value are both called \
                         `{value_name}` — they are two different types, so \
                         they need two different names"
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Parses one `hook_state!`/`hook_parameter!`/`otxn_parameter!` invocation's
/// argument tokens into a visibility, an entity, a key/name spec and a value
/// spec.
fn parse(input: TokenStream, role: Role) -> Result<Parsed, TokenStream> {
    let mac = role.macro_name();
    let mut c = Cursor::new(input);

    if c.is_empty() {
        return Err(err(
            Span::call_site(),
            &format!(
                "{mac}: expected an entity, a key/name and a value, e.g. {mac}(MyThing, Key => Value)"
            ),
        ));
    }

    let vis = parse_vis(&mut c);
    let entity = parse_entity(&mut c, role)?;
    let key = parse_key(&mut c, mac)?;

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

    check_distinct(&entity, &key, &value, mac)?;

    Ok(Parsed {
        vis,
        entity,
        key,
        value,
    })
}

/// One fully-parsed invocation.
struct Parsed {
    vis: Vis,
    entity: Entity,
    key: KeySpec,
    value: ValueSpec,
}

/// Recognizes and parses the `existing $Name = $bytes => ..` keyword form,
/// returning `None` when the leading `existing` is not the keyword at all
/// but an ordinary type the caller happens to have named `existing`.
///
/// `existing` is a *contextual* keyword: it only takes on its meaning when
/// directly followed by another identifier (the caller's type name). The
/// three continuations that keep a leading `existing` an ordinary type —
/// `existing => Value` (the pairing form), `existing::Path` and
/// `existing<..>` — are handed back to [`parse_key`] untouched. Everything
/// else after a leading `existing` is a malformed keyword form and gets its
/// own targeted diagnostic.
fn try_parse_existing(c: &mut Cursor, mac: &str) -> Option<Result<KeySpec, TokenStream>> {
    match c.peek() {
        Some(TokenTree::Ident(id)) if id.to_string() == "existing" => {}
        _ => return None,
    }
    if is_arrow_at(c, 1) || is_punct(c.peek_at(1), ':') || is_punct(c.peek_at(1), '<') {
        return None;
    }

    let name_span = c.peek_at(1).map_or_else(|| c.here_span(), TokenTree::span);
    c.bump(); // `existing`

    // A **path**, not an arbitrary token run: the caller's type may live
    // behind one (`owned::Locked`, `crate::keys::CfgName`), and this macro
    // only ever names it. Parsing the real grammar is what keeps
    // `existing b"CFG" = ..` (a literal where a type belongs) and
    // `existing CfgName => Config` (no bytes at all) from being swallowed
    // into a "name" that then fails deep inside generated code.
    //
    // Not naming-checked: the type is the caller's, so its spelling is their
    // business — a `snake_case` or raw-identifier marker is perfectly legal.
    let Some(name) = parse_path(c) else {
        return Some(Err(err(
            name_span,
            &format!(
                "{mac}: `existing` must be followed by the name of a type you \
                 declared yourself, e.g. `{mac}(Cfg, existing CfgName = \
                 b\"CFG\" => Config)`"
            ),
        )));
    };
    if !is_bare_eq(c, 0) {
        return Some(Err(err(name_span, &existing_form_shape(mac, &name))));
    }
    c.bump(); // `=`

    // Anchored before the collector runs: an empty or unterminated byte run
    // (`existing Name = => Ty`) must still get the targeted diagnostic for
    // *this* form, not the collector's generic one — the caller wrote
    // `existing`, so that is the shape they need explained.
    let bytes_span = c.here_span();
    Some(
        match collect_until_arrow(c, mac, "the fixed key/name bytes") {
            Ok(bytes) => Ok(KeySpec::ExistingFixed {
                name,
                name_span,
                bytes,
            }),
            Err(_) => Err(err(bytes_span, &existing_form_shape(mac, &name))),
        },
    )
}

/// Parses a type path — `ident (:: ident)*` — consuming exactly the tokens
/// that belong to it and leaving the cursor on whatever follows.
///
/// Only what this grammar actually needs to *name* a caller-owned type: no
/// leading `::`, no generic arguments, no `<T as Trait>` qualification. A
/// caller who needs one of those can alias it (`use` or `type`) and name the
/// alias, which is simpler for both sides than a general type parser here.
fn parse_path(c: &mut Cursor) -> Option<String> {
    let mut path = match c.peek() {
        Some(TokenTree::Ident(id)) => id.to_string(),
        _ => return None,
    };
    c.bump();

    // `::` reaches a proc macro as two separate `:` `Punct`s.
    while is_punct(c.peek_at(0), ':') && is_punct(c.peek_at(1), ':') {
        let Some(TokenTree::Ident(segment)) = c.peek_at(2) else {
            break;
        };
        let segment = segment.to_string();
        c.bump(); // `:`
        c.bump(); // `:`
        c.bump(); // segment
        path.push_str("::");
        path.push_str(&segment);
    }
    Some(path)
}

/// The one diagnostic every malformed `existing` tail gets, whatever went
/// wrong after the type name: this form accepts exactly one shape, so
/// naming that shape (and the way out of it) beats describing which token
/// was unexpected.
fn existing_form_shape(mac: &str, name: &str) -> String {
    format!(
        "{mac}: `existing {name}` accepts only the fixed-bytes shape \
         `= b\"...\" => Ty` — it attaches the key/name impls to a type you \
         declared yourself (the entity is still declared for you either \
         way). Drop `existing` to have {mac} declare `{name}` too, in any of \
         its four declaring forms."
    )
}

fn parse_key(c: &mut Cursor, mac: &str) -> Result<KeySpec, TokenStream> {
    if let Some(existing) = try_parse_existing(c, mac) {
        return existing;
    }

    let name_id = match c.peek() {
        // A literal directly after the entity's comma is never a key/name:
        // it is the old comma-form's byte string, or a stray argument. Say
        // what a declaration looks like rather than letting the token run be
        // collected as a "type" and fail deep inside rustc.
        Some(TokenTree::Literal(_)) => {
            return Err(err(
                c.here_span(),
                &format!(
                    "{mac}: expected a declaration after the entity name — one \
                     of `Name = b\"..\"`, `Key {{ .. }}`, `Key Inner`, \
                     `existing Name = b\"..\"`, or an existing key type"
                ),
            ));
        }
        Some(TokenTree::Ident(id)) => id.clone(),
        _ => {
            // Not a bare identifier at all (e.g. `[u8; 7]`) — can only be
            // an existing type used as-is.
            let ty_span = c.here_span();
            return Ok(KeySpec::Pairing {
                ty: collect_until_arrow(c, mac, "a key/name type")?,
                ty_span,
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
                                 (the fixed instance this declaration names)"
                            ),
                        ));
                    }
                    None => {
                        return Err(err(
                            c.end_span(),
                            &format!(
                                "{mac}: expected `{{ field: value, .. }}` after `=` \
                                 (the fixed instance this declaration names)"
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
        let bytes = collect_until_arrow(c, mac, "the fixed key/name bytes")?;
        return Ok(KeySpec::Fixed {
            name,
            name_span,
            bytes,
        });
    }

    // A second comma cannot begin any form: the only comma this grammar
    // accepts is the entity's, already consumed by `parse_entity`.
    if is_punct(c.peek_at(1), ',') {
        return Err(err(
            c.peek_at(1).map_or_else(Span::call_site, TokenTree::span),
            &format!(
                "{mac}: unexpected `,` after `{name_id}` — did you mean `=`? \
                 (e.g. `{mac}(MyThing, {name_id} = b\"...\" => Ty)`); only the \
                 entity name is comma-separated"
            ),
        ));
    }

    // `Name => ..` (the arrow starts immediately) — the pairing form, `Name`
    // a bare already-declared type.
    if is_arrow_at(c, 1) {
        let ty_span = name_id.span();
        c.bump(); // Name
        return Ok(KeySpec::Pairing {
            ty: name_id.to_string(),
            ty_span,
        });
    }

    // Anything immediately after the leading identifier that continues the
    // *same* type — `::` (a multi-segment path, e.g. `crate::MyKey`) or `<`
    // (generic args) — means this whole run is one already-declared type
    // (the pairing form): collect it verbatim.
    if is_punct(c.peek_at(1), ':') || is_punct(c.peek_at(1), '<') {
        let ty_span = c.here_span();
        return Ok(KeySpec::Pairing {
            ty: collect_until_arrow(c, mac, "a key/name type")?,
            ty_span,
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
    let inner_ty = collect_until_arrow(c, mac, "the type the newtype wraps")?;
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
    let ty_span = c.here_span();
    let mut rest: Vec<TokenTree> = Vec::new();
    while let Some(tt) = c.bump() {
        rest.push(tt);
    }
    Ok(ValueSpec::Existing {
        ty: tokens_to_string(&rest),
        ty_span,
    })
}

/// Renders a `struct Name;` (unit) or `struct Name(Inner);` (tuple)
/// declaration at `vis`, with the tuple field carrying that same visibility
/// so a `pub` entity is actually constructible from another module.
///
/// Every struct this macro declares carries two things beyond the fields:
///
/// - `#[allow(dead_code)]` — a hook that only touches the entity would
///   otherwise be warned about its declared key type, and one that only
///   writes state would be warned about the fields of its value type; both
///   are perfectly ordinary ways to use a declaration, and under this
///   workspace's `-D warnings` gate the warning is a build failure.
/// - a **doc comment**, on the item and on every field. Not decoration: the
///   optional leading visibility makes these `pub` items in the caller's
///   crate, and a crate that denies `missing_docs` (this workspace does)
///   would fail to build on an undocumented generated `pub` struct or field.
///   Since the macro is what declares them, the macro is what has to
///   document them.
fn tuple_or_unit_struct_decl(
    vis: &str,
    name: &str,
    inner_ty: Option<&str>,
    doc: &str,
    field_doc: &str,
) -> String {
    match inner_ty {
        Some(ty) => {
            format!("{doc}#[allow(dead_code)]\n{vis} struct {name}({field_doc}{vis} {ty});\n")
        }
        None => format!("{doc}#[allow(dead_code)]\n{vis} struct {name};\n"),
    }
}

/// Renders a named-field `struct Name { field: Type, .. }` declaration at
/// `vis` (fields included) from an already-parsed field list, each field
/// documented by name. See [`tuple_or_unit_struct_decl`] for why the doc
/// comments and `#[allow(dead_code)]` are here.
fn named_struct_decl(
    vis: &str,
    name: &str,
    fields: &[FieldShape],
    doc: &str,
    field_role: &str,
) -> String {
    let mut body = String::new();
    for f in fields {
        body.push_str(&doc_comment(&format!(
            "The `{field}` {field_role}.",
            field = f.name
        )));
        body.push_str(&format!(
            "{vis} {field}: {ty},\n",
            vis = vis,
            field = f.name,
            ty = f.ty
        ));
    }
    format!("{doc}#[allow(dead_code)]\n{vis} struct {name} {{\n{body}}}\n")
}

/// Renders `text` as a one-line doc comment, ready to prefix an item or a
/// field.
///
/// Deliberately plain prose with no `[`link`]` syntax: a generated doc
/// comment lands in the *caller's* crate, where an intra-doc link to a path
/// that resolves here may not resolve there — a broken-link warning the
/// caller cannot fix and did not write.
fn doc_comment(text: &str) -> String {
    format!("/// {text}\n")
}

/// Generates the key/name-side `ToBytes` + pairing-trait-supertrait impls
/// for a field-based struct (Forms 2/3/4) — [`hook_key::generate`] for
/// [`Role::State`], [`param_name::generate`] for a parameter role.
///
/// Called twice per struct-shaped invocation: once for the declared key/name
/// type, once for the **entity**, whose mirrored fields make it the identical
/// codegen problem. That is what keeps the entity zero-cost — it is not a
/// wrapper delegating to a key value, it *is* the same encoding.
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
/// type Value = {value}; ..}` (parameters).
///
/// For the parameter case, also overrides
/// [`TypedParamName::with_name_bytes`](::rshooks::convert::TypedParamName::with_name_bytes)
/// to allocate exactly `<{key} as ToBytes>::MAX_LEN` bytes instead of
/// relying on the trait's generic default (a full
/// [`PARAM_NAME_MAX_LEN`](::rshooks::convert::PARAM_NAME_MAX_LEN)
/// scratch buffer — see that method's doc comment for why the default
/// can't size itself this precisely). This works here because the
/// allocation sits inside a concrete, non-generic `impl` block — `{key}`
/// is a real type at codegen time, not a generic parameter — so
/// `MAX_LEN` is an ordinary compile-time constant.
///
/// That precision is also the reason the parameter arm re-emits
/// [`param_name::param_name_length_assert`] for `{key}`: overriding
/// `with_name_bytes` *replaces* the trait default's own
/// `1..=PARAM_NAME_MAX_LEN` assertion, so without a monomorphized copy here,
/// a caller-authored `ToBytes` name type reaching this impl through the
/// pairing form (whose type this macro never declared and never checked)
/// would get an unbounded `[0u8; MAX_LEN]` scratch buffer — a 0-byte name
/// the host rejects at runtime, or a multi-kilobyte stack buffer, with no
/// compile-time complaint at all. Forms 2–4 additionally get this same
/// assertion from their own [`param_name::generate`] codegen; a duplicate
/// anonymous `const _: ()` item is free (they are never named, so they
/// cannot collide) and is far cheaper than making the assertion's presence
/// depend on which form reached this function.
fn pairing_impl(key: &str, value: &str, role: Role) -> String {
    match role {
        Role::State => format!(
            "
#[automatically_derived]
impl ::rshooks::state::TypedStateKey for {key} {{
    type Value = {value};
}}
"
        ),
        Role::HookParam | Role::OtxnParam => format!(
            "
{length_assert}
#[automatically_derived]
impl ::rshooks::convert::TypedParamName for {key} {{
    type Value = {value};

    #[inline(always)]
    fn with_name_bytes<__R>(&self, f: impl ::core::ops::FnOnce(&[u8]) -> __R) -> __R {{
        let mut __buf = [0u8; <{key} as ::rshooks::convert::ToBytes>::MAX_LEN];
        let _ = ::rshooks::convert::ToBytes::write(self, &mut __buf);
        f(&__buf)
    }}
}}
",
            length_assert = param_name::param_name_length_assert(key),
        ),
    }
}

/// Generates the fixed-byte-string `ToBytes` impl the two fixed-bytes forms
/// (Form 1 and the `existing` keyword form) share for a key or name:
/// `MAX_LEN = { $bytes.len() }`, `write` delegating straight to `$bytes`'s
/// own `ToBytes::write` — the wire encoding *is* the in-memory
/// representation, nothing to compute (see
/// `rshooks::convert::TypedParamName`'s doc comment, "Zero-cost for
/// the plain-byte-string case").
fn fixed_bytes_to_bytes_impl(name: &str, bytes: &str) -> String {
    format!(
        "
#[automatically_derived]
impl ::rshooks::convert::ToBytes for {name} {{
    const MAX_LEN: usize = {{ ({bytes}).len() }};

    #[inline(always)]
    fn write(&self, buf: &mut [u8]) -> usize {{
        ::rshooks::convert::ToBytes::write({bytes}, buf)
    }}
}}
"
    )
}

/// Generates the parameter-role pairing for a fixed-byte-string name (Form
/// 1 or the `existing` keyword form): overrides
/// [`TypedParamName::with_name_bytes`](::rshooks::convert::TypedParamName::with_name_bytes)
/// to hand `f` `$bytes` directly — a `'static` reference, no copy, no
/// buffer, no per-call encode — the zero-copy fast path
/// `rshooks::convert::TypedParamName`'s doc comment describes.
fn fixed_bytes_param_pairing(name: &str, bytes: &str, value: &str) -> String {
    format!(
        "
#[automatically_derived]
impl ::rshooks::convert::TypedParamName for {name} {{
    type Value = {value};

    #[inline(always)]
    fn with_name_bytes<__R>(&self, f: impl ::core::ops::FnOnce(&[u8]) -> __R) -> __R {{
        f({bytes})
    }}
}}
"
    )
}

/// Generates every role impl a **fixed-bytes** type needs (Form 1's key, the
/// `existing` form's caller-owned name, and the entity of both) — the one
/// place that pairing of impls is spelled, so the entity and the key/name
/// provably get the same encoding.
fn fixed_bytes_role_impls(name: &str, bytes: &str, value: &str, role: Role) -> String {
    let mut src = fixed_bytes_to_bytes_impl(name, bytes);
    match role {
        Role::State => {
            src.push_str(&hook_key::state_key_encode_impl(name));
            src.push_str(&pairing_impl(name, value, role));
        }
        Role::HookParam | Role::OtxnParam => {
            src.push_str(&param_name::param_name_length_assert(name));
            src.push_str(&fixed_bytes_param_pairing(name, bytes, value));
        }
    }
    src
}

/// Generates the pairing form's entity impls: `struct E(K)` forwarding
/// through `&self.0` to whatever role traits `K` already has.
///
/// Deliberately **not** [`key_struct_codegen`] over a synthetic one-field
/// shape, which would require `K: ToBytes` and re-encode it field-wise. The
/// state side forwards [`StateKeyEncode::encode`](::rshooks::state::StateKeyEncode::encode)
/// directly, so a `state_keys!` enum — which has `StateKeyEncode` but no
/// `ToBytes` — pairs just as well as a `#[derive(HookKey)]` struct. The
/// parameter side forwards `with_name_bytes` to `K`'s own implementation
/// rather than re-deriving one, preserving whatever `K` already had: a
/// zero-copy `'static` literal, an exact-size scratch buffer, and the
/// monomorphized length assertion that comes with either.
///
/// Neither path constructs a `K`; both borrow the one the caller put in the
/// entity. That is what makes the form work for a non-`Copy` `K`.
fn pairing_entity_impls(entity: &str, key_ty: &str, value: &str, role: Role) -> String {
    match role {
        Role::State => format!(
            "
#[automatically_derived]
impl ::rshooks::state::StateKeyEncode for {entity} {{
    #[inline(always)]
    fn encode(&self) -> ::rshooks::state::EncodedStateKey {{
        <{key_ty} as ::rshooks::state::StateKeyEncode>::encode(&self.0)
    }}
}}
{pairing}
",
            pairing = pairing_impl(entity, value, role),
        ),
        Role::HookParam | Role::OtxnParam => format!(
            "
#[automatically_derived]
impl ::rshooks::convert::ToBytes for {entity} {{
    const MAX_LEN: usize = <{key_ty} as ::rshooks::convert::ToBytes>::MAX_LEN;

    #[inline(always)]
    fn write(&self, buf: &mut [u8]) -> usize {{
        ::rshooks::convert::ToBytes::write(&self.0, buf)
    }}
}}

{length_assert}
#[automatically_derived]
impl ::rshooks::convert::TypedParamName for {entity} {{
    type Value = {value};

    #[inline(always)]
    fn with_name_bytes<__R>(&self, f: impl ::core::ops::FnOnce(&[u8]) -> __R) -> __R {{
        <{key_ty} as ::rshooks::convert::TypedParamName>::with_name_bytes(&self.0, f)
    }}
}}
",
            length_assert = param_name::param_name_length_assert(entity),
        ),
    }
}

/// Generates the inherent accessor methods on the **entity**:
/// `get_state`/`set_state`/`update_state`/`delete_state` for [`Role::State`],
/// `get_value` (plus `get_name`, when `name_bytes` is `Some` — i.e. only the
/// two fixed-byte-string forms) for a parameter role.
///
/// Each method is a one-line forward to the free function that already
/// implements it, so `entity.get_state()` and `state_get_typed(&entity)`
/// compile to the same code — `#[inline(always)]` makes that structural
/// rather than hopeful, and the free functions stay the documented,
/// `_foreign`-capable surface. What the methods add is discoverability: a
/// declared entity answers `.` with exactly the operations its role
/// supports, instead of requiring the reader to already know which of ~12
/// `state_*` free functions applies.
///
/// These live on the entity and **only** on the entity. The key/name type is
/// a trait carrier; putting six method names on it would claim them on a
/// type the caller may well own (`existing`, pairing), and would make the
/// component look like the thing rather than the address of the thing.
///
/// Three attribute choices are load-bearing under this workspace's
/// `-D warnings` clippy gate:
///
/// - `#[automatically_derived]` sits on the **impl block**, not on the
///   methods: on a method it is an `unused_attributes` error.
/// - `#[allow(dead_code)]` sits on each **method**: a hook that declares an
///   entity and only ever writes it would otherwise fail the build over the
///   three accessors it did not call.
/// - Every path is fully qualified from the crate root (`::rshooks::..`,
///   `::core::..`) so the expansion cannot be captured by whatever the
///   caller happens to have in scope at the invocation site.
///
/// No generated body ever matches a specific [`HookError`] variant — every
/// one of them either forwards a `Result` unchanged or `map`s it (see
/// DESIGN.md §5.1 for why a specific-variant match inside inlinable code
/// drags the whole ~44-arm error decode into the caller's block nesting).
///
/// `get_name` is deliberately **not** generated for composite names (Forms
/// 2–4, pairing) or for state entities at all: a composite name has no
/// stored bytes to "get" — encoding it is a runtime computation whose result
/// would have to be copied into an owned array — so `with_name_bytes` (a
/// closure over an exact-size scratch buffer) remains the access path there.
/// A state key's bytes are an implementation detail of `StateKeyEncode` with
/// no caller-facing use at all.
fn accessor_impl(entity: &str, value: &str, role: Role, name_bytes: Option<&str>) -> String {
    let body = match role {
        Role::State => format!(
            "
    /// Reads this entity's hook-state entry, decoded as the value type it
    /// was declared with.
    ///
    /// `Ok(None)` means no entry exists for this key (never an error) — the
    /// method form of `rshooks::state::state_get_typed(&self)`, which it
    /// forwards to unchanged.
    #[allow(dead_code)]
    #[inline(always)]
    pub fn get_state(&self) -> ::rshooks::error::Result<::core::option::Option<{value}>> {{
        ::rshooks::state::state_get_typed(self)
    }}

    /// Writes this entity's hook-state entry, returning the number of
    /// bytes written.
    ///
    /// The method form of `rshooks::state::state_set_typed(&self, value)`,
    /// which it forwards to unchanged. `value` is taken by reference (and
    /// not cached inside the entity) so the entity stays a pure address: the
    /// one source of truth for the stored value remains the ledger.
    #[allow(dead_code)]
    #[inline(always)]
    pub fn set_state(&self, value: &{value}) -> ::rshooks::error::Result<usize> {{
        ::rshooks::state::state_set_typed(self, value)
    }}

    /// Read-modify-writes this entity's hook-state entry: reads the current
    /// value (`None` if absent), calls `f` for the next one, writes it back,
    /// and returns the number of bytes written.
    ///
    /// The method form of `rshooks::state::state_update_typed(&self, f)`,
    /// which it forwards to unchanged.
    #[allow(dead_code)]
    #[inline(always)]
    pub fn update_state(
        &self,
        f: impl ::core::ops::FnOnce(::core::option::Option<{value}>) -> {value},
    ) -> ::rshooks::error::Result<usize> {{
        ::rshooks::state::state_update_typed(self, f)
    }}

    /// Deletes this entity's hook-state entry (and refunds its reserve).
    ///
    /// The method form of `rshooks::state::state_delete(&self)`, which it
    /// forwards to unchanged — see that function's doc comment for why
    /// deletion is stated as its own operation rather than left to a
    /// `set_state` of some value that happens to encode to nothing.
    #[allow(dead_code)]
    #[inline(always)]
    pub fn delete_state(&self) -> ::rshooks::error::Result<()> {{
        ::rshooks::state::state_delete(self)
    }}
"
        ),
        Role::HookParam | Role::OtxnParam => {
            let (accessor, which) = match role {
                Role::OtxnParam => (
                    "::rshooks::api::otxn::otxn_param_typed",
                    "the originating transaction's own `HookParameters`",
                ),
                _ => (
                    "::rshooks::api::hook_ctx::hook_param_typed",
                    "this hook's own installed `HookParameters`",
                ),
            };
            let get_name = name_bytes.map_or_else(String::new, |bytes| {
                format!(
                    "
    /// This parameter's name bytes, exactly as declared — the same
    /// `'static` slice the `TypedParamName` impl hands to
    /// `with_name_bytes`, at no runtime cost (there is nothing to encode: a
    /// fixed byte string's wire form *is* its in-memory form).
    ///
    /// Only the fixed-byte-string forms have this method; a composite name
    /// has no stored bytes to hand back.
    ///
    /// Meant for **passing on and tracing** — handing the name to a raw
    /// `hook_param`/`otxn_param` call, or to `trace!` — not for comparing.
    /// Do not compare the result with `==`: slice equality can compile to
    /// an unguarded `bcmp` loop (`rshooks::buf_eq`'s module doc comment
    /// explains why that matters, and its fixed-length `buf_eq_N` helpers
    /// are the loop-free answer *for fixed-size arrays* — they do not take
    /// the `&[u8]` this returns, so there is nothing to point a
    /// slice-length comparison at here). A name you declared is a compile-
    /// time constant anyway: match on the declaration, not on its bytes.
    #[allow(dead_code)]
    #[inline(always)]
    pub const fn get_name(&self) -> &'static [u8] {{
        {bytes}
    }}
"
                )
            });
            format!(
                "{get_name}
    /// Reads this parameter out of {which}, decoded as the value type it was
    /// declared with.
    ///
    /// The method form of `{accessor}(&self)`, which it forwards to
    /// unchanged — including which of `hook_param`/`otxn_param` is read,
    /// fixed once at the declaration by the macro that declared this entity.
    #[allow(dead_code)]
    #[inline(always)]
    pub fn get_value(&self) -> ::rshooks::error::Result<{value}> {{
        {accessor}(self)
    }}
"
            )
        }
    };

    format!(
        "
#[automatically_derived]
impl {entity} {{{body}}}
"
    )
}

/// Expands a parsed invocation into the full declaration — the key/name
/// type and its trait impls, the entity and *its* trait impls, an optional
/// inline value struct, and the entity's inherent accessors.
///
/// The value side is resolved *first* (it never depends on the key), so
/// that when the key and entity sides are processed, `value_ty` is already
/// available and every pairing impl can be finished in one pass with no
/// placeholder bookkeeping.
fn generate(parsed: Parsed, role: Role) -> TokenStream {
    let Parsed {
        vis,
        entity,
        key,
        value,
    } = parsed;
    let vis = vis.text.as_str();
    let entity = entity.name;

    let mut src = String::new();
    // Anchors the "internal codegen failed to parse" fallback below at a
    // real token from the invocation, updated as each declared name is
    // seen, rather than always pointing at the macro call site.
    let mut err_span = Span::call_site();

    let subject = role.subject();
    let component = role.component();
    let entity_doc = doc_comment(&format!(
        "Entity handle for the `{entity}` {subject}: the type its accessors          hang off."
    ));
    let key_doc = doc_comment(&format!(
        "The {component} component addressing the `{entity}` {subject}."
    ));

    let value_ty = match value {
        ValueSpec::Existing { ty, .. } => ty,
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
            src.push_str(&named_struct_decl(
                vis,
                &name,
                &shape.fields,
                &doc_comment(&format!("The value held by the `{entity}` {subject}.")),
                "field of the stored value",
            ));
            src.push_str(&value_struct_codegen(&shape, role));
            name
        }
    };

    // The bytes a fixed-form entity hands to `get_name`, if any.
    let mut name_bytes: Option<String> = None;

    match key {
        KeySpec::Pairing { ty, .. } => {
            // The caller's key type gets the pairing it was named for; the
            // entity wraps and forwards to it.
            src.push_str(&pairing_impl(&ty, &value_ty, role));
            src.push_str(&tuple_or_unit_struct_decl(
                vis,
                &entity,
                Some(&ty),
                &entity_doc,
                &doc_comment(&format!("The `{ty}` this entity addresses.")),
            ));
            src.push_str(&pairing_entity_impls(&entity, &ty, &value_ty, role));
        }
        KeySpec::ExistingFixed {
            name,
            name_span,
            bytes,
        } => {
            // `existing`: the caller declared `name` themselves, so only its
            // impls are generated. The entity encodes the same literal
            // directly — it never constructs `name`, which may not even be
            // constructible from here.
            err_span = name_span;
            src.push_str(&fixed_bytes_role_impls(&name, &bytes, &value_ty, role));
            src.push_str(&tuple_or_unit_struct_decl(
                vis,
                &entity,
                None,
                &entity_doc,
                "",
            ));
            src.push_str(&fixed_bytes_role_impls(&entity, &bytes, &value_ty, role));
            name_bytes = role.is_param().then(|| bytes.clone());
        }
        KeySpec::Fixed {
            name,
            name_span,
            bytes,
        } => {
            err_span = name_span;
            src.push_str(&tuple_or_unit_struct_decl(vis, &name, None, &key_doc, ""));
            src.push_str(&fixed_bytes_role_impls(&name, &bytes, &value_ty, role));
            src.push_str(&tuple_or_unit_struct_decl(
                vis,
                &entity,
                None,
                &entity_doc,
                "",
            ));
            src.push_str(&fixed_bytes_role_impls(&entity, &bytes, &value_ty, role));
            name_bytes = role.is_param().then(|| bytes.clone());
        }
        KeySpec::Struct {
            name,
            name_span,
            fields,
            init,
        } => {
            err_span = name_span;
            let key_shape = StructShape {
                name: name.clone(),
                name_span,
                fields: fields.clone(),
            };
            let field_role = format!("component of this {component}");
            src.push_str(&named_struct_decl(
                vis,
                &name,
                &key_shape.fields,
                &key_doc,
                &field_role,
            ));
            src.push_str(&key_struct_codegen(&key_shape, role));
            src.push_str(&pairing_impl(&name, &value_ty, role));

            // The entity mirrors the key's fields and encodes *itself*
            // through the identical per-field codegen — same layout, same
            // cost, no key value ever built.
            let entity_shape = StructShape {
                name: entity.clone(),
                name_span,
                fields,
            };
            src.push_str(&named_struct_decl(
                vis,
                &entity,
                &entity_shape.fields,
                &entity_doc,
                &field_role,
            ));
            src.push_str(&key_struct_codegen(&entity_shape, role));
            src.push_str(&pairing_impl(&entity, &value_ty, role));

            if let Some(init) = init {
                // Form 2's fixed instance is a `const` of the *entity's*
                // name — the entity is what the accessors hang off, so the
                // one canonical instance should be one too. A type and a
                // value may share a name (separate namespaces).
                src.push_str(&format!(
                    "{doc}#[allow(dead_code, non_upper_case_globals)]\n\
                     {vis} const {entity}: {entity} = {entity} {{ {init} }};\n",
                    doc = doc_comment(&format!(
                        "The one fixed `{entity}` instance this declaration names."
                    )),
                ));
            }
        }
        KeySpec::Newtype {
            name,
            name_span,
            inner_ty,
        } => {
            err_span = name_span;
            src.push_str(&tuple_or_unit_struct_decl(
                vis,
                &name,
                Some(&inner_ty),
                &key_doc,
                &doc_comment(&format!("The wrapped {component}.")),
            ));
            let key_shape = StructShape {
                name: name.clone(),
                name_span,
                fields: vec![FieldShape {
                    name: "0".to_string(),
                    ty: inner_ty.clone(),
                }],
            };
            src.push_str(&key_struct_codegen(&key_shape, role));
            src.push_str(&pairing_impl(&name, &value_ty, role));

            src.push_str(&tuple_or_unit_struct_decl(
                vis,
                &entity,
                Some(&inner_ty),
                &entity_doc,
                &doc_comment(&format!("The wrapped {component}.")),
            ));
            let entity_shape = StructShape {
                name: entity.clone(),
                name_span,
                fields: vec![FieldShape {
                    name: "0".to_string(),
                    ty: inner_ty,
                }],
            };
            src.push_str(&key_struct_codegen(&entity_shape, role));
            src.push_str(&pairing_impl(&entity, &value_ty, role));
        }
    }

    src.push_str(&accessor_impl(
        &entity,
        &value_ty,
        role,
        name_bytes.as_deref(),
    ));

    src.parse::<TokenStream>().unwrap_or_else(|_| {
        err(
            err_span,
            &format!(
                "rshooks-macros: internal {} codegen failed to parse",
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
        Ok(parsed) => generate(parsed, role),
        Err(e) => e,
    }
}
