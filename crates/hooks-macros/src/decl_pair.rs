//! Shared parser + codegen backing the `hook_state!`/`hook_parameter!`/
//! `otxn_parameter!` declaration-macro grammar — a "staircase" of four
//! *declaring* forms (plus two forms that declare no type of their own:
//! the `existing` keyword form and the two-type pairing form), from a
//! fully-fixed key/name down to a fully composite, runtime-constructed
//! one:
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
//! // `existing` keyword form: impls only, attached to a key/name type the
//! // caller declared itself (so it can carry its own visibility, derives
//! // and docs). Module position only.
//! struct MyOwnKey;
//! hook_state!(existing MyOwnKey = b"MK" => u64);
//!
//! // Pairing form: pairs two already-declared types.
//! hook_state!(SomeExistingKey => SomeExistingValue);
//! ```
//!
//! Every *declaring* form (1–4 and `existing`) additionally gets a small
//! set of inherent accessor methods on the key/name type it declares —
//! `get_state`/`set_state`/`update_state`/`delete_state` for a state key,
//! `get_value` (+ `get_name` for the two fixed-byte-string forms) for a
//! parameter name — so a declaration and its accesses read as one object
//! rather than as a type plus a free function taking it (see
//! [`accessor_impl`]). The plain two-type pairing form deliberately gets
//! none: it does not declare the type, so growing inherent methods on it
//! would silently claim six method names on a type this macro does not own.
//!
//! Form 1 and a struct form carrying an explicit initializer — the two
//! declaring shapes that can name a complete instance — may be prefixed by
//! an optional **instance binder**, a `snake_case` identifier and a comma,
//! which additionally binds one instance of the declared key/name to a
//! local variable, making the whole invocation statement-position sugar for
//! the self-contained case (every other form rejects a binder with a
//! diagnostic naming the way out — see [`Binder`] and [`check_binder_decl`]):
//!
//! ```text
//! hook_state!(deposit_state, DepositKey {tag: u8, owner: AccountId}
//!             = {tag: 1, owner: sender} => Deposit {amount: u64});
//! deposit_state.owner = someone_else;          // `let mut`
//! deposit_state.set_state(&Deposit { amount: 1 })?;
//! ```
//!
//! The value side (after `=>`) independently accepts either an
//! already-declared type name (as above) or an *inline* definition —
//! `=> Name { field: Type, .. }` — which generates a fresh
//! `#[derive(HookData)]`-equivalent (state role) or
//! `#[derive(ParamValue)]`-equivalent (parameter role) struct named `Name`.
//!
//! `hook_parameter!`/`otxn_parameter!` accept the identical grammar,
//! targeting `hooks_lib::convert::TypedParamName` instead of
//! `hooks_lib::state::TypedStateKey`.
//!
//! # Why one shared module for three macros
//!
//! `hook_state!`'s key side and `hook_parameter!`/`otxn_parameter!`'s name
//! side play near-identical roles (see `hooks_lib::state::TypedStateKey`'s
//! doc comment's comparison table with `hooks_lib::convert::TypedParamName`)
//! — same grammar staircase, same per-field codegen shape (reused directly
//! from [`crate::hook_key`]/[`crate::param_name`]), differing only in which
//! trait gets paired (`StateKeyEncode`+`TypedStateKey` vs. a
//! `1..=32`-bound-checked `TypedParamName`) and in which inherent accessors
//! the declared type gets (a key can be read, written, updated and deleted;
//! a parameter name is read-only from the reading hook's perspective).
//! Parsing and struct/field codegen live here, once; [`Role`] parameterizes
//! the small remaining difference at the two places it actually matters
//! (the key/name-side trait impl, and the accessor block).
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
use proc_macro::{Delimiter, Group, Ident, Punct, Spacing, Span, TokenStream, TokenTree};

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

/// An optional **instance binder**: the `snake_case` identifier (plus the
/// comma separating it from the declaration) an invocation may lead with,
/// asking for one instance of the declared key/name to be bound to a local
/// variable on top of the ordinary declaration.
///
/// Only the *declaring* forms that can name a complete instance accept one
/// — Form 1 (the type's own name *is* the instance) and Form 2/3 with an
/// explicit `= { field: value, .. }` initializer. The remaining forms are
/// rejected with a form-specific diagnostic in [`check_binder_decl`], which
/// also explains why each one cannot have a binder.
///
/// Because a binder makes the expansion "items plus a `let`", a binder
/// invocation is **statement position only**, and the types it declares are
/// function-local: that is the intended envelope (one hook function owning
/// its key, value and accesses). Sharing a key/name type across functions
/// or module `const`s keeps using the (unchanged) non-binder forms.
struct Binder {
    /// The caller's **original** identifier token, already validated as
    /// `snake_case` and not a Rust keyword by [`check_binder_ident`].
    ///
    /// Kept as a token, not just as text, because it is re-emitted verbatim
    /// into the generated `let`: a binder that arrives through a caller's
    /// own `macro_rules!` wrapper (`hook_state!($binder, ..)`) carries that
    /// caller's syntax context, and local bindings *are* hygienic — rebuild
    /// the identifier from a string and the `let` introduces a variable in
    /// the wrapper macro's context instead, invisible to the code that
    /// asked for it (verified: doing so fails the wrapper-macro test in
    /// `crates/hooks-lib/tests/decl_instance.rs` with E0425).
    ident: Ident,
    /// The identifier's text, for diagnostics.
    name: String,
    /// Span of the identifier, for diagnostics that have no better anchor
    /// (a binder combined with a form that declares nothing).
    span: Span,
}

/// The key/name half of a parsed declaration.
enum KeySpec {
    /// Backward-compatible: an already-declared type, used as the key/name
    /// as-is (`ExistingKey => ..`). Nothing is declared for it here — and,
    /// deliberately, no inherent accessors either (see the module doc
    /// comment): this macro does not own that type.
    Existing { ty: String },
    /// The `existing` keyword form, `existing $Name = $bytes => $Ty` —
    /// `$Name` is a key/name type the caller declared separately (e.g.
    /// `pub struct CfgName;`, carrying its own visibility/derives/docs), so
    /// (unlike the declaring forms below) no struct is declared for it, and
    /// its name is not naming-checked (this macro did not declare it). It
    /// *is* an explicit opt-in — the caller invoked the macro on their own
    /// type — so it gets the full set of impls the fixed-bytes Form 1 gets,
    /// inherent accessors included.
    ExistingFixed {
        name: String,
        name_span: Span,
        bytes: String,
    },
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
    /// name (or, with an instance binder, bound to the binder's local
    /// variable instead).
    Struct {
        name: String,
        name_span: Span,
        fields: Vec<FieldShape>,
        init: Option<StructInit>,
    },
    /// Form 4: `$Name $Inner => ..` — declares `$Name` as a new tuple
    /// struct wrapping the single existing type `$Inner`.
    Newtype {
        name: String,
        name_span: Span,
        inner_ty: String,
    },
}

/// A Form-2/binder struct initializer (`= { field: value, .. }`), kept in
/// **both** representations because its two consumers need genuinely
/// different things from it:
///
/// - `text` — the initializer re-rendered as source, spliced into the
///   `const $Name: $Name = $Name { .. };` Form 2 declares. That path has
///   always been a string one (the whole `const` item is assembled as
///   source and re-parsed), and stays one: a `const` initializer is a
///   compile-time expression with nothing to capture from the surrounding
///   scope, so losing the caller's spans there costs nothing but a slightly
///   less precise error span.
/// - `group` — the caller's **original** brace [`Group`], re-emitted
///   token-for-token when an instance binder turns the initializer into a
///   runtime `let $binder = $Name { .. };`. Here the initializer is an
///   ordinary expression that may name locals, call functions, or (via a
///   wrapper `macro_rules!`) carry identifiers from a different syntax
///   context — all of which a stringify/re-parse round trip would strip of
///   their hygiene and spans, turning a working wrapper macro into an
///   unresolved-identifier error. Splicing the original `Group` keeps every
///   inner token exactly as the caller wrote it.
struct StructInit {
    /// The initializer's contents rendered back to source text (Form 2's
    /// `const` path).
    text: String,
    /// The caller's original brace group (the instance-binder `let` path).
    group: Group,
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
/// every token run this grammar takes verbatim — an "existing type" the
/// macro does not declare, a newtype's wrapped type, or a fixed form's name
/// bytes.
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
/// Only applied to names this macro itself declares ([`KeySpec::Existing`]/
/// [`KeySpec::ExistingFixed`] reference a type the *caller* already named,
/// which this macro has no business re-validating — a caller's own marker
/// type may perfectly well be spelled `snake_case` or as a raw identifier).
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

/// Rust's reserved words — the strict keyword set plus every word reserved
/// for future use, across all editions this crate can be compiled in
/// (edition 2024's `gen` included). None of them can name a local variable,
/// so none of them can be an instance binder; rejecting them here produces a
/// diagnostic that names the rule, instead of rustc's own (much more
/// confusing) parse error inside a macro expansion the caller cannot see.
///
/// Weak keywords (`union`, `macro_rules`, `'static`, ...) are deliberately
/// absent: they are ordinary identifiers in binding position and make
/// perfectly usable binder names.
const RUST_KEYWORDS: &[&str] = &[
    // Strict keywords (2015/2018/2021/2024).
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while", // Reserved for future use (`gen` is edition 2024's).
    "abstract", "become", "box", "do", "final", "gen", "macro", "override", "priv", "try",
    "typeof", "unsized", "virtual", "yield",
];

/// Validates an instance-binder identifier: `[a-z][a-z0-9_]*`, not a lone
/// `_`, not a Rust keyword.
///
/// The mirror image of [`check_upper_camel_case`] on the type side, and for
/// the same reason: the binder names a **local variable**, so anything but
/// `snake_case` would draw rustc's own `non_snake_case` warning from inside
/// a macro expansion, where the caller has no offending token to look at.
/// Raw identifiers are handled by [`parse_binder`] before this function is
/// reached (`r#fn` would otherwise pass the keyword check by spelling).
fn check_binder_ident(name: &str, span: Span, mac: &str) -> Result<(), TokenStream> {
    if name == "_" {
        return Err(err(
            span,
            &format!(
                "{mac}: `_` is not an instance binder — give the instance a \
                 name (e.g. `{mac}(deposit_state, ..)`), or leave the binder \
                 out entirely if the declaration is all you want"
            ),
        ));
    }
    let first_ok = matches!(name.chars().next(), Some(ch) if ch.is_ascii_lowercase());
    let rest_ok = name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_');
    if !first_ok || !rest_ok {
        return Err(err(
            span,
            &format!(
                "{mac}: `{name}` is not snake_case — an instance binder names \
                 a local variable, so it must start with a lowercase ASCII \
                 letter and contain only lowercase ASCII letters, digits and \
                 underscores (e.g. `acct_param`, not `acctParam` or `_acct`). \
                 To declare a type instead, drop the comma: `{mac}(Name = \
                 b\"..\" => Ty)`"
            ),
        ));
    }
    if RUST_KEYWORDS.contains(&name) {
        return Err(err(
            span,
            &format!(
                "{mac}: `{name}` is a Rust keyword (or reserved for a future \
                 edition) and cannot name a local variable — pick another \
                 instance binder"
            ),
        ));
    }
    Ok(())
}

/// Parses the optional leading `$binder ,` — an identifier followed directly
/// by a comma, and nothing else in this grammar looks like that.
///
/// Every rejection routes to the *most specific* diagnostic available, in
/// this order (each check would otherwise be swallowed by a later, vaguer
/// one):
///
/// 1. A **raw identifier** (`r#fn`) — recognized by its `r#` display prefix,
///    before any other check, so `r#fn` is rejected as a raw identifier
///    rather than sailing past the keyword list by spelling.
/// 2. `existing` — reserved, because `existing, Name = ..` would otherwise
///    parse as a perfectly valid binder literally named `existing` and
///    silently drop the caller's intended `existing` keyword form.
/// 3. Anything else — validated as a binder by [`check_binder_ident`].
fn parse_binder(c: &mut Cursor, mac: &str) -> Result<Option<Binder>, TokenStream> {
    let id = match c.peek() {
        Some(TokenTree::Ident(id)) if is_punct(c.peek_at(1), ',') => id.clone(),
        _ => return Ok(None),
    };
    let span = id.span();
    let name = id.to_string();

    if name.starts_with("r#") {
        return Err(err(
            span,
            &format!(
                "{mac}: `{name}` is a raw identifier and cannot be an instance \
                 binder — an instance binder is a plain snake_case name (e.g. \
                 `{mac}(deposit_state, ..)`)"
            ),
        ));
    }
    if name == "existing" {
        return Err(err(
            span,
            &format!(
                "{mac}: `existing` declares impls for your own type at module \
                 scope and cannot be an instance binder — write `{mac}(existing \
                 YourType = b\"..\" => Ty)` (no comma), then bind it locally \
                 with `let your_name = YourType;` if you want a local name"
            ),
        ));
    }
    check_binder_ident(&name, span, mac)?;

    c.bump(); // binder
    c.bump(); // `,`
    Ok(Some(Binder {
        ident: id,
        name,
        span,
    }))
}

/// Rejects an instance binder combined with a declaration that cannot
/// produce one bound instance, with a diagnostic naming the way out for that
/// specific form.
///
/// Every rejection here is a *scoping* or *definite-initialization* issue,
/// not an arbitrary restriction — see each arm's message.
fn check_binder_decl(binder: &Binder, key: &KeySpec, mac: &str) -> Result<(), TokenStream> {
    let b = &binder.name;
    match key {
        // Form 1 (the type's own name is the instance) and Form 2/3 *with*
        // an initializer are exactly the forms that can name a complete
        // instance.
        KeySpec::Fixed { .. } => Ok(()),
        KeySpec::Struct { init: Some(_), .. } => Ok(()),
        KeySpec::Struct {
            name, name_span, ..
        } => Err(err(
            *name_span,
            &format!(
                "{mac}: give every field an initial value: `{name} {{ .. }} = \
                 {{ field: value, .. }}` — an instance binder never leaves key \
                 fields uninitialized (a half-built key would silently store \
                 under an all-zero ledger key)"
            ),
        )),
        // The `existing` form emits impls for a type the *module* owns; from
        // inside a function body those would be non-local impls, and two
        // functions declaring the same one would collide.
        KeySpec::ExistingFixed {
            name, name_span, ..
        } => Err(err(
            *name_span,
            &format!(
                "{mac}: an instance binder cannot be combined with `existing` — \
                 declare `existing {name} = ..` once at module scope, then bind \
                 locally with `let {b} = {name};`"
            ),
        )),
        // A newtype's instance needs the inner value, which this grammar has
        // no place to spell.
        KeySpec::Newtype {
            name, name_span, ..
        } => Err(err(
            *name_span,
            &format!(
                "{mac}: an instance binder cannot be combined with the newtype \
                 form — construct `{name}(..)` yourself (`let {b} = \
                 {name}(inner);`)"
            ),
        )),
        // Nothing is declared, so there is nothing for the binder to be an
        // instance *of* that the caller could not already write themselves.
        KeySpec::Existing { ty } => Err(err(
            binder.span,
            &format!(
                "{mac}: an instance binder needs a form that declares the \
                 key/name — `{ty}` is already declared elsewhere, so bind it \
                 with an ordinary `let {b} = ..;` instead"
            ),
        )),
    }
}

/// Parses one `hook_state!`/`hook_parameter!`/`otxn_parameter!` invocation's
/// argument tokens into an optional instance binder, a key/name spec and a
/// value spec.
fn parse(
    input: TokenStream,
    role: Role,
) -> Result<(Option<Binder>, KeySpec, ValueSpec), TokenStream> {
    let mac = role.macro_name();
    let mut c = Cursor::new(input);

    if c.is_empty() {
        return Err(err(
            Span::call_site(),
            &format!("{mac}: expected a key/name and a value, e.g. {mac}(Key => Value)"),
        ));
    }

    let binder = parse_binder(&mut c, mac)?;
    let key = parse_key(&mut c, mac)?;
    if let Some(binder) = &binder {
        check_binder_decl(binder, &key, mac)?;
    }

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

    Ok((binder, key, value))
}

/// Recognizes and parses the `existing $Name = $bytes => ..` keyword form,
/// returning `None` when the leading `existing` is not the keyword at all
/// but an ordinary type the caller happens to have named `existing`.
///
/// `existing` is a *contextual* keyword: it only takes on its meaning when
/// directly followed by another identifier (the caller's type name). The
/// three continuations that keep a leading `existing` an ordinary type —
/// `existing => Value` (the two-type pairing form), `existing::Path` and
/// `existing<..>` — are handed back to [`parse_key`] untouched, so no
/// pre-existing invocation changes meaning. Everything else after a leading
/// `existing` is a malformed keyword form and gets its own targeted
/// diagnostic rather than the binder one.
fn try_parse_existing(c: &mut Cursor, mac: &str) -> Option<Result<KeySpec, TokenStream>> {
    match c.peek() {
        Some(TokenTree::Ident(id)) if id.to_string() == "existing" => {}
        _ => return None,
    }
    if is_arrow_at(c, 1) || is_punct(c.peek_at(1), ':') || is_punct(c.peek_at(1), '<') {
        return None;
    }

    let name_id = match c.peek_at(1) {
        Some(TokenTree::Ident(id)) => id.clone(),
        _ => {
            return Some(Err(err(
                c.here_span(),
                &format!(
                    "{mac}: `existing` must be followed by the name of a type \
                     you declared yourself, e.g. `{mac}(existing CfgName = \
                     b\"CFG\" => Config)`"
                ),
            )));
        }
    };
    // Not naming-checked: `existing` names a type the *caller* declared, so
    // its spelling is the caller's business — a `snake_case` or raw-identifier
    // marker type is perfectly legal here.
    let name = name_id.to_string();
    let name_span = name_id.span();
    c.bump(); // `existing`
    c.bump(); // Name

    if !is_bare_eq(c, 0) {
        return Some(Err(err(c.here_span(), &existing_form_shape(mac, &name))));
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

/// The one diagnostic every malformed `existing` tail gets, whatever went
/// wrong after the type name: this form accepts exactly one shape, so
/// naming that shape (and the way out of it) beats describing which token
/// was unexpected.
fn existing_form_shape(mac: &str, name: &str) -> String {
    format!(
        "{mac}: `existing {name}` accepts only the fixed-bytes shape \
         `= b\"...\" => Ty` — it attaches key/name bytes and a value \
         pairing to a type you declared yourself, and declares nothing \
         of its own (drop `existing` to have {mac} declare `{name}` for \
         you, in any of its four declaring forms)"
    )
}

fn parse_key(c: &mut Cursor, mac: &str) -> Result<KeySpec, TokenStream> {
    if let Some(existing) = try_parse_existing(c, mac) {
        return existing;
    }

    let name_id = match c.peek() {
        Some(TokenTree::Ident(id)) => id.clone(),
        _ => {
            // Not a bare identifier at all (e.g. `[u8; 7]`) — can only be
            // an existing type used as-is.
            return Ok(KeySpec::Existing {
                ty: collect_until_arrow(c, mac, "a key/name type")?,
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
                        Some(StructInit {
                            text: tokens_to_string(&inits),
                            group: g,
                        })
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
        let bytes = collect_until_arrow(c, mac, "the fixed key/name bytes")?;
        return Ok(KeySpec::Fixed {
            name,
            name_span,
            bytes,
        });
    }

    // A comma here cannot begin any form: the only comma this grammar
    // accepts is the instance binder's, and `parse_binder` has already
    // consumed (or rejected) it before this function runs — so reaching one
    // here means a second binder.
    if is_punct(c.peek_at(1), ',') {
        return Err(err(
            c.peek_at(1).map_or_else(Span::call_site, TokenTree::span),
            &format!(
                "{mac}: unexpected `,` after `{name_id}` — did you mean `=`? \
                 (e.g. `{mac}({name_id} = b\"...\" => Ty)`); at most one \
                 leading snake_case instance binder may precede the declaration"
            ),
        ));
    }

    // `Name => ..` (the arrow starts immediately) — the two-type pairing
    // form, `Name` a bare already-declared type.
    if is_arrow_at(c, 1) {
        c.bump(); // Name
        return Ok(KeySpec::Existing {
            ty: name_id.to_string(),
        });
    }

    // Anything immediately after the leading identifier that continues the
    // *same* type — `::` (a multi-segment path, e.g. `crate::MyKey`) or `<`
    // (generic args) — means this whole run is one existing, more complex
    // type (the two-type pairing form): collect it verbatim.
    if is_punct(c.peek_at(1), ':') || is_punct(c.peek_at(1), '<') {
        return Ok(KeySpec::Existing {
            ty: collect_until_arrow(c, mac, "a key/name type")?,
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
/// Form-2/3/4 and two-type pairing declaration uses.
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
///
/// That precision is also the reason the parameter arm re-emits
/// [`param_name::param_name_length_assert`] for `{key}`: overriding
/// `with_name_bytes` *replaces* the trait default's own
/// `1..=PARAM_NAME_MAX_LEN` assertion, so without a monomorphized copy here,
/// a caller-authored `ToBytes` name type reaching this impl through the
/// two-type pairing form (`hook_parameter!(MyName => MyValue)`, whose
/// `MyName` this macro never declared and never checked) would get an
/// unbounded `[0u8; MAX_LEN]` scratch buffer — a 0-byte name the host
/// rejects at runtime, or a multi-kilobyte stack buffer, with no compile-time
/// complaint at all. Forms 2–4 additionally get this same assertion from
/// their own [`param_name::generate`] codegen; a duplicate anonymous `const
/// _: ()` item is free (they are never named, so they cannot collide) and is
/// far cheaper than making the assertion's presence depend on which form
/// reached this function.
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
{length_assert}
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
",
            length_assert = param_name::param_name_length_assert(key),
        ),
    }
}

/// Generates the fixed-byte-string `ToBytes` impl the two fixed-bytes forms
/// (Form 1 and the `existing` keyword form) share for a key or name: `MAX_LEN = { $bytes.len() }`, `write` delegating
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
/// 1 or the `existing` keyword form): overrides
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

/// Generates the inherent accessor methods every **declaring** form gets on
/// the key/name type it declares: `get_state`/`set_state`/`update_state`/
/// `delete_state` for [`Role::State`], `get_value` (plus `get_name`, when
/// `name_bytes` is `Some` — i.e. only the two fixed-byte-string forms) for a
/// parameter role.
///
/// Each method is a one-line forward to the free function that already
/// implements it, so `key.get_state()` and `state_get_typed(&key)` compile
/// to the same code — `#[inline(always)]` makes that structural rather than
/// hopeful, and the free functions stay the documented, `_foreign`-capable
/// surface. What the methods add is discoverability: a declared key answers
/// `.` with exactly the operations its role supports, instead of requiring
/// the reader to already know which of ~12 `state_*` free functions applies.
///
/// Three attribute choices are load-bearing under this workspace's
/// `-D warnings` clippy gate:
///
/// - `#[automatically_derived]` sits on the **impl block**, not on the
///   methods: on a method it is an `unused_attributes` error.
/// - `#[allow(dead_code)]` sits on each **method**: a hook that declares a
///   key and only ever writes it would otherwise fail the build over the
///   three accessors it did not call.
/// - Every path is fully qualified from the crate root (`::hooks_lib::..`,
///   `::core::..`) so the expansion cannot be captured by whatever the
///   caller happens to have in scope at the invocation site.
///
/// No generated body ever matches a specific [`HookError`] variant — every
/// one of them either forwards a `Result` unchanged or `map`s it (see
/// DESIGN.md §5.1 for why a specific-variant match inside inlinable code
/// drags the whole ~44-arm error decode into the caller's block nesting).
///
/// `get_name` is deliberately **not** generated for composite names (Forms
/// 2–4) or for state keys at all: a composite name has no stored bytes to
/// "get" — encoding it is a runtime computation whose result would have to
/// be copied into an owned array, inviting `==` comparisons that compile to
/// an unguarded `bcmp` loop — so `with_name_bytes` (a closure over an
/// exact-size scratch buffer) remains the access path there, and
/// `hooks_lib::buf_eq*` remains the way to compare. A state key's bytes are
/// an implementation detail of `StateKeyEncode` with no caller-facing use at
/// all.
fn accessor_impl(key: &str, value: &str, role: Role, name_bytes: Option<&str>) -> String {
    let body = match role {
        Role::State => format!(
            "
    /// Reads this key's own hook-state entry, decoded as the value type it
    /// was declared with.
    ///
    /// `Ok(None)` means no entry exists for this key (never an error) — the
    /// method form of `hooks_lib::state::state_get_typed(&self)`, which it
    /// forwards to unchanged.
    #[allow(dead_code)]
    #[inline(always)]
    pub fn get_state(&self) -> ::hooks_lib::error::Result<::core::option::Option<{value}>> {{
        ::hooks_lib::state::state_get_typed(self)
    }}

    /// Writes this key's own hook-state entry, returning the number of
    /// bytes written.
    ///
    /// The method form of `hooks_lib::state::state_set_typed(&self, value)`,
    /// which it forwards to unchanged. `value` is taken by reference (and
    /// not cached inside the key) so the key stays a pure address: the one
    /// source of truth for the stored value remains the ledger.
    #[allow(dead_code)]
    #[inline(always)]
    pub fn set_state(&self, value: &{value}) -> ::hooks_lib::error::Result<usize> {{
        ::hooks_lib::state::state_set_typed(self, value)
    }}

    /// Read-modify-writes this key's own hook-state entry: reads the
    /// current value (`None` if absent), calls `f` for the next one, writes
    /// it back, and returns the number of bytes written.
    ///
    /// The method form of `hooks_lib::state::state_update_typed(&self, f)`,
    /// which it forwards to unchanged.
    #[allow(dead_code)]
    #[inline(always)]
    pub fn update_state(
        &self,
        f: impl ::core::ops::FnOnce(::core::option::Option<{value}>) -> {value},
    ) -> ::hooks_lib::error::Result<usize> {{
        ::hooks_lib::state::state_update_typed(self, f)
    }}

    /// Deletes this key's own hook-state entry (and refunds its reserve).
    ///
    /// The method form of `hooks_lib::state::state_delete(&self)`, which it
    /// forwards to unchanged — see that function's doc comment for why
    /// deletion is stated as its own operation rather than left to a
    /// `set_state` of some value that happens to encode to nothing.
    #[allow(dead_code)]
    #[inline(always)]
    pub fn delete_state(&self) -> ::hooks_lib::error::Result<()> {{
        ::hooks_lib::state::state_delete(self)
    }}
"
        ),
        Role::HookParam | Role::OtxnParam => {
            let (accessor, which) = match role {
                Role::OtxnParam => (
                    "::hooks_lib::api::otxn::otxn_param_typed",
                    "the originating transaction's own `HookParameters`",
                ),
                _ => (
                    "::hooks_lib::api::hook_ctx::hook_param_typed",
                    "this hook's own installed `HookParameters`",
                ),
            };
            let get_name = name_bytes.map_or_else(String::new, |bytes| {
                format!(
                    "
    /// This name's own bytes, exactly as declared — the same `'static`
    /// slice the `TypedParamName` impl hands to `with_name_bytes`, at no
    /// runtime cost (there is nothing to encode: a fixed byte string's wire
    /// form *is* its in-memory form).
    ///
    /// Only the fixed-byte-string forms have this method; a composite name
    /// has no stored bytes to hand back.
    ///
    /// Meant for **passing on and tracing** — handing the name to a raw
    /// `hook_param`/`otxn_param` call, or to `trace!` — not for comparing.
    /// Do not compare the result with `==`: slice equality can compile to
    /// an unguarded `bcmp` loop (`hooks_lib::buf_eq`'s module doc comment
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
    /// Reads the parameter this name refers to out of {which}, decoded as
    /// the value type it was declared with.
    ///
    /// The method form of `{accessor}(&self)`, which it forwards to
    /// unchanged — including which of `hook_param`/`otxn_param` is read,
    /// fixed once at the declaration by the macro that declared this name.
    #[allow(dead_code)]
    #[inline(always)]
    pub fn get_value(&self) -> ::hooks_lib::error::Result<{value}> {{
        {accessor}(self)
    }}
"
            )
        }
    };

    format!(
        "
#[automatically_derived]
impl {key} {{{body}}}
"
    )
}

/// The instance-binder `let` statement's leading tokens, up to (not
/// including) the binder identifier itself.
///
/// The `#[allow(..)]` attributes are mandatory, not defensive: under this
/// workspace's `-D warnings` gate an unused binder would otherwise fail the
/// build (`unused_variables`), and so would a struct binder whose fields are
/// never reassigned (`unused_mut`) — both of which are perfectly reasonable
/// ways to use this macro. `mut` is only for the struct form: re-aiming a
/// composite key between accesses is the point of it, while Form 1's
/// zero-sized instance has nothing to assign to.
fn binder_let_head(has_init: bool) -> &'static str {
    if has_init {
        "#[allow(unused_variables, unused_mut)] let mut"
    } else {
        "#[allow(unused_variables)] let"
    }
}

/// Expands a parsed `(binder, key, value)` triple into the full declaration
/// — struct(s), field-based codegen, the pairing impl, the inherent
/// accessors, and (with a binder) the trailing `let`.
///
/// The value side is resolved *first* (it never depends on the key), so
/// that when the key side is processed, `value_ty` is already available —
/// letting [`KeySpec::Fixed`]/[`KeySpec::ExistingFixed`] (the two forms whose
/// pairing impl embeds the value type directly, rather than deferring to
/// the shared [`pairing_impl`]) finish in one pass with no placeholder
/// bookkeeping.
fn generate(binder: Option<Binder>, key: KeySpec, value: ValueSpec, role: Role) -> TokenStream {
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
    // with `pairing_impl` at the end; `Fixed`/`ExistingFixed` instead finish
    // their own (non-uniform) pairing impl immediately, since state and
    // parameters need genuinely different bodies for them (see
    // `fixed_bytes_param_pairing`'s doc comment).
    let mut key_ty: Option<String> = None;
    // The declared key/name type the inherent accessors attach to — set by
    // the *declaring* arms only. Deliberately separate from `key_ty` above:
    // the two-type pairing arm sets `key_ty` (it does need the pairing
    // impl) but must never appear here, or the macro would claim six method
    // names on a type it did not declare (see the module doc comment, and
    // the `existing_pairing_no_methods` ui fixture that pins it).
    let mut accessors_for: Option<String> = None;
    // What an instance binder, if present, binds: the declared type's name
    // plus (struct forms) the caller's original `= { field: value, .. }`
    // group, kept out of the string-built expansion so it can be spliced in
    // token-for-token (see `StructInit`). Only the arms `check_binder_decl`
    // accepts ever set it.
    let mut binder_target: Option<BinderTarget> = None;

    match key {
        KeySpec::Existing { ty } => key_ty = Some(ty),
        KeySpec::ExistingFixed {
            name,
            name_span,
            bytes,
        } => {
            // `existing Name = bytes => Ty`: `Name` is declared *elsewhere*
            // by the caller — only the impls are generated. The accessors
            // are generated too: invoking the macro *on* one's own type is
            // as explicit an opt-in as a declaring form is.
            err_span = name_span;
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
            src.push_str(&accessor_impl(
                &name,
                &value_ty,
                role,
                role.is_param().then_some(bytes.as_str()),
            ));
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
            src.push_str(&accessor_impl(
                &name,
                &value_ty,
                role,
                role.is_param().then_some(bytes.as_str()),
            ));
            // The unit struct's own name *is* its one instance.
            binder_target = Some(BinderTarget { name, init: None });
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
            accessors_for = Some(name.clone());
            match (init, binder.is_some()) {
                // Form 2 without a binder: the fixed instance becomes a
                // `const` of the struct's own name, as it always has.
                (Some(init), false) => src.push_str(&format!(
                    "#[allow(non_upper_case_globals)]\nconst {name}: {name} = {name} {{ {init} }};\n",
                    init = init.text,
                )),
                // With a binder the initializer is a *runtime* expression
                // bound to the binder's local instead — no same-named
                // `const` is declared (it would demand a const-evaluable
                // initializer, defeating the point of the binder form).
                (Some(init), true) => {
                    binder_target = Some(BinderTarget {
                        name: name.clone(),
                        init: Some(init.group),
                    });
                }
                (None, _) => {}
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
            accessors_for = Some(name.clone());
            key_ty = Some(name);
        }
    }

    if let Some(key_ty) = key_ty {
        src.push_str(&pairing_impl(&key_ty, &value_ty, role));
    }
    if let Some(accessors_for) = accessors_for {
        // Composite forms have no fixed name bytes to hand back, so they
        // get no `get_name` (see `accessor_impl`'s doc comment).
        src.push_str(&accessor_impl(&accessors_for, &value_ty, role, None));
    }

    let expansion = src.parse::<TokenStream>().unwrap_or_else(|_| {
        err(
            err_span,
            &format!(
                "hooks-macros: internal {} codegen failed to parse",
                role.macro_name()
            ),
        )
    });

    match (binder, binder_target) {
        (Some(binder), Some(target)) => {
            append_binder_let(expansion, &binder, &target, err_span, role)
        }
        // A binder with no target would mean `check_binder_decl` accepted a
        // form the `match key` above declares nothing for — an internal
        // inconsistency, reported as one rather than silently dropping the
        // caller's binding.
        (Some(_), None) => err(
            err_span,
            &format!(
                "hooks-macros: internal {} codegen has no instance to bind",
                role.macro_name()
            ),
        ),
        (None, _) => expansion,
    }
}

/// What an instance binder binds: the name of the declared type, plus the
/// caller's original initializer group for the struct forms (Form 1's unit
/// struct needs none — its type name is its one value).
struct BinderTarget {
    /// The declared key/name type, as spelled at the invocation.
    name: String,
    /// The caller's `= { field: value, .. }` group, re-emitted verbatim.
    init: Option<Group>,
}

/// Appends the instance binder's `let` statement to an already-built
/// expansion, **token-wise**.
///
/// Only the statement's fixed scaffolding (the `#[allow(..)] let mut` head
/// and the `= $Name` that follows the binder) is built from source text.
/// The two pieces that came from the caller are spliced in as the caller's
/// own tokens:
///
/// - the **binder identifier**, because a `let` binding is hygienic: an
///   identifier rebuilt from a string binds in whatever syntax context this
///   proc macro was invoked from, which for a caller's `macro_rules!`
///   wrapper is that wrapper's own context — the variable would then be
///   invisible to the code that asked for it (E0425, at the caller's line);
/// - a struct initializer's **brace group**, so every token inside it keeps
///   the span and syntax context it was written with. That is what lets a
///   wrapper `macro_rules!` pass one of *its* locals into the initializer.
///
/// The declared type's name is the one piece that is safely rebuildable:
/// Rust's `macro_rules!` hygiene covers local variables and labels, not
/// items, so a re-created type identifier resolves to the struct this same
/// expansion declared.
fn append_binder_let(
    expansion: TokenStream,
    binder: &Binder,
    target: &BinderTarget,
    err_span: Span,
    role: Role,
) -> TokenStream {
    let head = binder_let_head(target.init.is_some());
    let assign = format!("= {name}", name = target.name);
    let (Ok(head), Ok(assign)) = (head.parse::<TokenStream>(), assign.parse::<TokenStream>())
    else {
        return err(
            err_span,
            &format!(
                "hooks-macros: internal {} instance-binder codegen failed to parse",
                role.macro_name()
            ),
        );
    };

    let mut out = expansion;
    out.extend(head);
    out.extend([TokenTree::Ident(binder.ident.clone())]);
    out.extend(assign);
    if let Some(init) = &target.init {
        out.extend([TokenTree::Group(init.clone())]);
    }
    out.extend([TokenTree::Punct(Punct::new(';', Spacing::Alone))]);
    out
}

/// Entry point invoked by `hook_state!`/`hook_parameter!`/`otxn_parameter!`
/// in `lib.rs` (one thin `#[proc_macro]` wrapper per macro name, `role`
/// pinning which one), mirroring the `#[proc_macro_derive]`/`derive(..)`
/// split the four struct derives already use.
pub(crate) fn expand(input: TokenStream, role: Role) -> TokenStream {
    match parse(input, role) {
        Ok((binder, key, value)) => generate(binder, key, value, role),
        Err(e) => e,
    }
}
