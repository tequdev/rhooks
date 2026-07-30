//! Every way an instance-binder identifier can be rejected.
//!
//! One fixture rather than seven: each invocation fails independently during
//! expansion, so rustc reports all of them in a single compilation and the
//! pinned `.stderr` covers the whole rule at once.
//!
//! A binder names a **local variable**, so it must be a plain `snake_case`
//! identifier. Anything else would surface as rustc's own `non_snake_case`
//! warning (or a parse error) from inside an expansion the caller never
//! wrote.

use hooks_lib::hook_state;

fn main() {
    // camelCase.
    hook_state!(acctParam, AcctKey = b"AK" => u64);

    // UpperCamelCase — a type name, not a variable name. The trailing hint
    // points at the form that declares a type instead.
    hook_state!(CfgName, CfgKey = b"CK" => u64);

    // A strict keyword.
    hook_state!(type, TypeKey = b"TK" => u64);

    // Reserved for a future edition (`gen` is edition 2024's).
    hook_state!(gen, GenKey = b"GK" => u64);

    // A raw identifier — caught by its `r#` prefix before the keyword list,
    // which it would otherwise slip past by spelling.
    hook_state!(r#fn, RawKey = b"RK" => u64);

    // A lone `_` binds nothing; drop the binder instead.
    hook_state!(_, UnderscoreKey = b"UK" => u64);

    // `existing` is reserved in binder position: without that, this would
    // parse as a valid binder named `existing` and silently discard the
    // caller's intended `existing` keyword form.
    hook_state!(existing, ExistingKey = b"EK" => u64);
}
