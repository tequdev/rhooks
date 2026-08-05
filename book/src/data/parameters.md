# Hook and Transaction Parameters

Hooks read configuration from two distinct sources, both called
"parameters" but attached at very different times: **hook parameters**,
set once when the hook is installed (via `SetHook`), and **originating
transaction parameters**, attached fresh by whoever submits the triggering
transaction. This page covers both — the loose byte-buffer accessors, the
`hook_parameter!`/`otxn_parameter!` entity macros that pair a name with a
value type, and composite (struct-shaped) names via `#[derive(ParamName)]`.

## Two sources, one shape

| | Hook parameter | Otxn parameter |
|---|---|---|
| set by | the hook's operator, at `SetHook` time | whoever submits the triggering transaction |
| read with | `hook_param`/`hook_param_exact`/`hook_param_typed` | `otxn_param`/`otxn_param_exact`/`otxn_param_typed` |
| entity macro | `hook_parameter!` | `otxn_parameter!` |
| typical use | operator-controlled configuration (a minimum amount, a blocklist entry, a pause switch) | per-invocation instructions the caller supplies |

Both mechanisms are read-only from the reading hook's own perspective — a
hook parameter is set by whoever installs the hook, not written by the hook
itself at runtime (`hook_param_set` exists, but it writes a *different*
hook's parameter, taking a raw `&[u8]`, not a typed value — out of scope
for this page). Because of that, both sides share the exact same
`TypedParamName` trait and accessor shape; only the underlying host call
differs.

## The loose accessors

`hook_param`/`otxn_param` read a named parameter into a caller-provided
buffer, mirroring `otxn_field`'s shape (see [Reading the Originating
Transaction](otxn.md)):

```rust
use rshooks::prelude::*;

let mut buf = [0u8; 32];
let written = hook_param(&mut buf, b"CFG")?;
```

`hook_param_exact`/`otxn_param_exact` require the parameter to be exactly
`T`'s length (any `FixedRead` type), with `T` inferred from context, not a
turbofish. `examples/03_hook-params` uses exactly this for a compiled-in
default when the operator hasn't configured a minimum:

```rust
const MIN_PARAM: &[u8] = b"MIN";
const DEFAULT_MIN_DROPS: u64 = 1_000_000;

fn min_drops() -> u64 {
    hook_param_exact(MIN_PARAM)
        .map(u64::from_be_bytes)
        .unwrap_or(DEFAULT_MIN_DROPS)
}
```

`hook_param_exact`'s return type is inferred as `[u8; 8]` from the
`.map(u64::from_be_bytes)` call — no turbofish needed. Note the
`from_be_bytes`, not this crate's `FromBytes` trait: a raw parameter byte
buffer is whatever the caller who set it chose to write, conventionally
matching Xahau Binary's big-endian numeric encoding for a value like this,
the same convention [Reading the Originating Transaction](otxn.md)
describes for raw protocol fields. `.unwrap_or(DEFAULT_MIN_DROPS)`
collapses "not configured at all" and "configured with a value of the
wrong size" into the same fallback, without treating a malformed parameter
as a hard error.

## The typed pairing: `hook_param_typed`/`otxn_param_typed`

`hook_param_exact`/`otxn_param_exact` take the name and the value type `T`
as two *independent* arguments — nothing stops calling
`otxn_param_exact::<WrongType>(b"INS")` for a name/type combination that
was never intended, as long as `WrongType: FixedRead` (true of nearly
every fixed-size type this crate provides, including some *other*
parameter's value type).

`TypedParamName` closes that gap: implement it for a name type — directly,
or via `hook_parameter!`/`otxn_parameter!` — to declare its one paired
value type once, then call `hook_param_typed`/`otxn_param_typed` with **a
reference to a name value**. The return type is resolved from the name
argument itself, never a turbofish and never an independently-chosen type
a mismatch could hide behind.

## The entity macros: `hook_parameter!`/`otxn_parameter!`

These declare a parameter **entity** — the thing your hook reads — using
the identical **grammar staircase** `hook_state!` uses for hook state (see
[Hook State](state.md)):

| form | name shape | example |
|---|---|---|
| 1 | fully fixed (a new zero-sized type) | `hook_parameter!(Cfg, CfgName = b"CFG" => Config);` |
| 2 | struct, with a fixed instance | same shape as `hook_state!`, applied to a name instead of a key |
| 3 | struct, constructed per call site | `hook_parameter!(SeatVote, SeatParamName {topic: u8, seat: u8} => Vote);` |
| 4 | newtype (tuple struct) around one existing type | same shape as `hook_state!` |
| `existing` | name impls on a name type **you** declared | `hook_parameter!(Cfg, existing CfgName = b"CFG" => Config);` |
| pairing | wraps a name type you already declared, that already encodes | `hook_parameter!(SeatVote, SeatParamName => Vote);` |

`otxn_parameter!` has the exact same grammar; the only difference is which
host call the generated `get_value()` forwards to (`hook_param_typed` vs.
`otxn_param_typed`), so the declaration site itself documents which of the
two a parameter is meant for.

### Form 1, with the compiled-in-default pattern

`examples/03_hook-params`'s `MIN` parameter, expressed the typed way:

```rust
use rshooks::prelude::*;
use rshooks::{ParamValue, hook_parameter};

hook_parameter!(Cfg, CfgName = b"CFG" => Config {min_amount: u64});

fn min_amount() -> u64 {
    Cfg.get_value().map(|c| c.min_amount).unwrap_or(1_000_000)
}
```

Form 1 declares both `Cfg` (the entity) and `CfgName` (the name component)
as new zero-sized types, both encoding the literal `b"CFG"`. `Cfg.get_name()
-> &'static [u8]` is available as a `const fn` returning that literal
directly — no encode step at all — because a plain byte-string name has
nothing to compute: its wire encoding *is* its in-memory representation.
`Cfg.get_value()` is an `#[inline(always)]` forward to
`hook_param_typed(&CfgName)`; the two spellings compile to the same code,
so the choice is purely readability.

`.unwrap_or(default)` is the idiomatic way to give a hook a sensible
compiled-in fallback: `get_value()` returns `Err` uniformly whether the
parameter was never set or was set with the wrong byte length, so one
`unwrap_or` handles "unconfigured" and "malformed" the same way, exactly
like the loose `hook_param_exact` pattern above.

## Composite names: `#[derive(ParamName)]`

A Hook API parameter name isn't always a plain literal tag — per the Hook
API itself it's a genuine variable-length key of up to 32 bytes, and (like
a hook state key) can be a whole composite, struct-shaped value instead of
a byte string. `#[derive(ParamName)]` derives `ToBytes` (write-only — see
below) for a named-field struct used this way. `examples/12_typed-data`'s
`typed-data` hook uses this for an operator-controlled pause switch, the
same idea `xahaud`'s own genesis governance hook uses for its `IS0`..`IS19`
seat parameters:

```rust
use rshooks::prelude::*;
use rshooks::{ParamName, ParamValue, hook_parameter};

#[derive(ParamName, Clone, Copy)]
struct AdminName {
    section: u8,
    field: u8,
}

hook_parameter!(AdminPause, AdminName => PauseSwitch {paused: u8});

const ADMIN_PAUSE: AdminPause = AdminPause(AdminName {
    section: 0,
    field: 0,
});

fn deposits_paused() -> bool {
    ADMIN_PAUSE
        .get_value()
        .map(|s| s.paused != 0)
        .unwrap_or(false)
}
```

This is `hook_parameter!`'s **pairing form** — an entity wrapping a name
type already declared with `#[derive(ParamName)]`, paired with an inline
`PauseSwitch` value (`#[derive(ParamValue)]`-equivalent codegen, generated
inline here). `AdminName` encodes to 2 bytes (`section` then `field`, no
padding), comfortably inside the Hook API's 1-to-32-byte parameter-name
bound. Unlike an oversized `HookData` state value (no size cap at all),
`#[derive(ParamName)]` checks this bound — both the 1-byte lower bound and
the 32-byte upper bound — **at the struct's own definition**, so an
out-of-range name fails to compile before it's ever used.

Because `AdminName` is composite rather than a fixed byte string, its
`TypedParamName` impl has to actually encode at runtime — laying `section`
and `field` out into a small buffer sized exactly to `AdminName::MAX_LEN`
(not the full 32-byte scratch the trait's generic default would need,
since only a concrete, non-generic impl can size an array by an associated
constant). `examples/12_typed-data`'s README measures this directly: +29
worst-case instructions over the same hook without the composite name,
versus the near-zero cost of the plain `CFG`/`INS` tags used elsewhere in
that same hook.

## Why the typed pairing prevents name/value mismatches

The loose `hook_param_exact::<T>(name)`/`otxn_param_exact::<T>(name)` calls
take `name` and `T` as two independent arguments — a typo or a copy-paste
error can pair the right name with the wrong type, or the wrong name with
the right type, and both compile fine as long as `T: FixedRead`. Every
`hook_parameter!`/`otxn_parameter!` declaration removes that degree of
freedom: the name type is permanently tied to exactly one value type
(`TypedParamName::Value`), so `Cfg.get_value()` and `Ins.get_value()` in
`examples/12_typed-data` can never accidentally decode one parameter's
bytes as the other's struct shape — the compiler resolves the return type
from the entity itself, with no independently-chosen type left for a
mismatch to hide in. This is the identical safety property [Hook
State](state.md)'s `hook_state!` gives the key/value side; see [Typed Data
with Derives](typed-data.md) for the underlying `ParamName`/`ParamValue`
derives both macros build on.
