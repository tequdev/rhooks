# Hook State

A Hook's persistent storage is a flat key-value store scoped to the
account it's installed on (and, for a foreign read, another account's
namespace too). `rshooks` gives you three tiers of access to it, from a raw
buffer read all the way up to a one-line declaration that generates typed
accessor methods. This page walks through all three, plus reading another
account's state. If you haven't read [Typed Data with Derives](typed-data.md)
yet, the `#[derive(HookKey)]`/`#[derive(HookData)]` derives it covers are
what the higher tiers here are built on.

## The state model: 32-byte keys, host-side left-padding

Every state entry is addressed by a key of up to 32 bytes. The Hook API
accepts a key from 1 to 32 bytes and **left-pads a shorter key internally**
to its own fixed-width storage slot — the same idiom a C hook uses when it
calls `state(&v, 8, "RR", 2)` with a 2-byte literal key. `rshooks` mirrors
this at every layer: a short key is sent to the host at its own real,
unpadded length, never locally zero-padded to 32 bytes. Values, by
contrast, are read and written as plain bytes with no implied structure —
interpreting them is entirely up to the layer you're using.

## Tier 1: the loose, single-value API

`rshooks::api::state` (re-exported by the prelude) is the lowest-level
typed convenience over the raw `state`/`state_set` host calls: a family of
small functions for exactly the primitive cases, each taking a raw
`&[u8]`-like key with no key-type story of its own.

```rust
use rshooks::prelude::*;

let mut buf = [0u8; 32];
let key = [0u8; 32];
let written = state(&mut buf, &key)?;

state_set(&buf[..written], &key)?;
```

For the common primitive shapes there are dedicated helpers —
`state_u32`/`state_set_u32`, `state_i64`/`state_set_i64`,
`state_xfl`/`state_set_xfl`, and their `state_update_*` read-modify-write
counterparts — all little-endian via `state_exact` under the hood. The one
outlier is `state_u64`/`state_update_u64`, which use the host's as-int64
mode and read/write **big-endian** — intended for an entry whose bytes
originated from Xahau Binary itself (a protocol-mirroring value, or interop
with a C hook), not one this crate's own typed layer wrote. For a
little-endian `u64` written by the typed layer, use `state_u64_le` instead.
`state_exact::<T>` is the general fixed-length escape hatch this tier is
built on, identical in spirit to `otxn_field_exact` (see [Reading the
Originating Transaction](otxn.md)): `T` must be exactly the right length,
inferred from context, no turbofish.

Reach for this tier for a one-off primitive read/write with no reuse
value. For a hook with more than a couple of distinct state entries, the
next two tiers pay off quickly.

## Tier 2: `state_keys!` — a typed key enum, independent value type

`crate::state`'s `state_get`/`state_set_loose`/`state_update_loose` work
for *any* type implementing `ToBytes`/`FromBytes` — not just the
primitives Tier 1 hard-codes — paired with a `state_keys!`-declared enum
for the key side:

```rust
use rshooks::prelude::*;
use rshooks::state_keys;

state_keys! {
    /// This hook's persistent data.
    enum DataKey {
        /// A running counter.
        Counter,
        /// A per-owner balance, keyed by the owner's account.
        Balance(AccountId),
    }
}

let count: Option<u64> = state_get(&DataKey::Counter)?;
state_set_loose(&DataKey::Counter, &1u64)?;
```

A unit variant (`Counter`) encodes to just its 1-byte discriminant, no
padding at all. A tuple variant (`Balance(AccountId)`) carries exactly one
`ToBytes` payload, encoded at runtime as "discriminant byte + payload,"
again with no trailing padding — the real length sent to the host is `1 +
Payload::MAX_LEN`. Declaration order matters: the macro assigns each
variant a sequential `u8` discriminant, so inserting or reordering a
variant changes every later variant's encoded key (and thus which on-chain
slot it addresses).

`state_get`/`state_set_loose`/`state_update_loose` still take the key and
the value type as *independent* generic parameters, though — nothing stops
calling `state_get::<SomeOtherType>(&DataKey::Counter)` for a pairing that
was never intended, as long as `SomeOtherType: FromBytes` (true of nearly
every fixed-size type this crate provides). That's exactly the gap Tier 3
closes.

## Tier 3: `hook_state!` — a key permanently paired with its value type

`hook_state!` declares a hook-state **entity**: a key bound to exactly one
value type, with four generated accessor methods
(`get_state`/`set_state`/`update_state`/`delete_state`) and a
`TypedStateKey` implementation that also makes it usable with every free
`state_get_typed`/`state_set_typed`/`state_update_typed` function. There is
no second, independently-chosen value type left for a mismatch to hide in
— passing the wrong value for a key is a compile error.

It offers a **grammar staircase** of six forms, from a fully-fixed key down
to a fully composite, runtime-constructed one. Pick the narrowest one that
fits:

| form | key shape | example |
|---|---|---|
| 1 | fully fixed (a new zero-sized type) | `hook_state!(RewardRate, RewardRateKey = b"RR" => XFL);` |
| 2 | struct, with a fixed instance | `hook_state!(Counter, CounterKey {name: [u8; 7]} = {name: *b"counter"} => u64);` |
| 3 | struct, constructed per call site | `hook_state!(DepositState, DepositKey {tag: u8, owner: AccountId} => Deposit);` |
| 4 | newtype (tuple struct) around one existing type | `hook_state!(AccountState, AccountKey AccountId => AccountData {balance: XFL, sequence: u16});` |
| `existing` | key impls on a key type **you** declared | `hook_state!(MyOwnState, existing MyOwnKey = b"MK" => u64);` |
| pairing | wraps a key type you already declared, that already encodes | `hook_state!(MyState, MyKey => MyValue);` |

Every form declares the **entity** first — the thing your hook operates
on, and the only thing that gets the four accessor methods. The **key**
component gets no methods of its own; it's a trait carrier you hand to the
free functions when you want the address rather than the thing addressed.
The value side (after `=>`) accepts either an already-declared type or an
inline definition (`=> Name { field: Type, .. }`), which generates a fresh
`#[derive(HookData)]`-equivalent struct.

### Form 1: fully fixed key

`$Entity` and `$Name` both become new unit structs — zero-sized markers
whose own name *is* the one value — encoding the same fixed, literal bytes
either way:

```rust
use rshooks::prelude::*;
use rshooks::hook_state;

hook_state!(RewardRate, RewardRateKey = b"RR" => XFL);

let current = RewardRate.get_state()?;
RewardRate.set_state(&XFL::one())?;
```

### Form 3: struct key, constructed per call site

Use this when the key varies at runtime — keyed by the calling account, for
example:

```rust
use rshooks::prelude::*;
use rshooks::hook_state;

hook_state!(DepositState, DepositKey {tag: u8, owner: AccountId} => Deposit {amount: u64, deadline: u32, flags: u8});

let deposit = DepositState { tag: 1, owner: AccountId::default() };
let current = deposit.get_state()?;
deposit.set_state(&Deposit { amount: 1, deadline: 0, flags: 0 })?;
```

`get_state`/`set_state`/`update_state`/`delete_state` are `#[inline(always)]`
forwards to `state_get_typed(&deposit)`/`state_set_typed(&deposit, &v)`/etc
— the method call and the free-function call compile to identical code, so
the choice is purely about which reads better at the call site.

### The pairing form: entities over derives you already wrote

When you've already declared `#[derive(HookKey)]`/`#[derive(HookData)]`
types yourself (see [Typed Data with Derives](typed-data.md)), the pairing
form ties them together without redeclaring anything:

```rust
use rshooks::prelude::*;
use rshooks::{hook_state, HookData, HookKey};

#[derive(HookKey, Clone, Copy)]
struct MyKey {
    tag: u8,
}

#[derive(HookData, Clone, Copy, Debug, PartialEq)]
struct MyValue {
    count: u32,
}

hook_state!(MyState, MyKey => MyValue);

let value = MyState(MyKey { tag: 0 }).get_state()?;
```

`$Key` must be local to your crate (Rust's orphan rule — a bare `[u8; N]`
or `types::StateKey` needs Form 4's newtype wrapper instead), already able
to encode itself (`StateKeyEncode`, from `#[derive(HookKey)]` or
`state_keys!`), and not already paired with another value type. A
`state_keys!` enum — which has `StateKeyEncode` but no `ToBytes` — pairs
just as well as a `#[derive(HookKey)]` struct, since the entity forwards
`encode()` straight through rather than re-deriving it.

For the remaining forms (2, 4, and `existing`) and every edge case — the
visibility rules, why deletion (`delete_state`) needs its own spelling
rather than an empty-value write, and the full compile-time error messages
for a misused pairing — see `rshooks::hook_state!`'s own rustdoc, which is
the canonical reference this section summarizes.

## The counter walkthrough

`examples/02_state-counter` is the smallest complete tutorial for the typed
layer, using Form 2 (a struct key with a fixed instance):

```rust
hook_state!(Counter, CounterKey {name: [u8; 7]} = {name: *b"counter"} => u64);

#[hook]
fn my_hook() -> i64 {
    let count = Counter.get_state().unwrap_or(Some(0)).unwrap_or(0);

    let next = count.wrapping_add(1);
    if Counter.set_state(&next).is_err() {
        rollback!(
            b"state-counter: state_set failed",
            StateCounterError::StateSetFailed
        );
    }

    accept!(b"state-counter: incremented", next as i64)
}
```

One line declares `Counter` (the entity, with the four accessors), `const
Counter: Counter = Counter { .. }` (the fixed instance — legal because a
type name and a value name live in separate namespaces), and `CounterKey`
(the key component). `Counter.get_state()` returns `Result<Option<u64>>`:
`Ok(None)` means "no entry yet" (see below), so the double `unwrap_or`
handles both "never written" and "an unexpected read error" the same way,
defaulting to zero either way.

`CounterKey { name: *b"counter" }` sends exactly the same 7 bytes a bare
`*b"counter"` array key would (see "Key length and padding" above) — the
struct wrapper exists only to satisfy the orphan rule for the generated
`TypedStateKey` impl, not to change what's on the wire.

## `Ok(None)` means "no entry" — never a special-cased error

Every typed read here maps "no entry for this key" to `Ok(None)`, the same
shape as `HashMap::get` — ordinary, not exceptional. Every *other* error,
including a present-but-undersized entry that fails to decode as `T`,
still comes back as `Err`, so a genuine decode failure is never mistaken
for "nothing was ever stored here."

## Deleting an entry

The Hook API has no dedicated "delete" call — an entry is deleted by
writing zero bytes to it, which also refunds the owner reserve it was
holding. `state_delete` (and the generated `delete_state()` method) is the
explicit spelling for that, independent of any value type — deliberately
not reachable by pairing a key with a value type that happens to encode to
nothing, which would spell "delete" as an accident of the value type
rather than an intent at the call site. `examples/12_typed-data` deletes a
depositor's record on full withdrawal for exactly this reason (releasing
the reserve, rather than leaving a zeroed entry behind):

```rust
if deposit.delete_state().is_err() {
    rollback!(
        b"typed-data: state_set failed",
        TypedDataError::StateSetFailed
    );
}
```

## Foreign state: reading another account's entries

`state_foreign`/`state_foreign_get`/`state_foreign_get_typed` (and their
`set`/`update` twins) read or write a state entry belonging to another
account, or another namespace on this hook's own account. `namespace` and
`account` both default to "this hook's own" when passed `None`; when
present, they're a bare reference (`&target`), not `Some(&target)` — a
generic `Option<...>` parameter can't also accept a bare `None` literal
without becoming ambiguous, so `rshooks` uses a small `ForeignRef` trait
instead that accepts either shape directly.

`examples/09_state-foreign` reads a flag from a target account configured
via a Hook parameter:

```rust
hook_parameter!(AcctParam, AcctParamName = b"ACCT" => AccountId);

const ENABLED_KEY: StateKey = StateKey(pad!(b"enabled"));

#[hook]
fn my_hook() -> i64 {
    let Ok(target) = AcctParam.get_value() else {
        rollback!(
            b"state-foreign: ACCT parameter not configured",
            StateForeignError::AcctNotConfigured
        )
    };

    let mut flag = [0u8; 1];
    match state_foreign(&mut flag, &ENABLED_KEY, None, &target) {
        Ok(n) if n == flag.len() => {}
        Err(HookError::DoesntExist) => rollback!(
            b"state-foreign: not configured on target account",
            StateForeignError::NotConfiguredOnTarget
        ),
        _ => rollback!(
            b"state-foreign: state_foreign read failed",
            StateForeignError::ReadFailed
        ),
    }

    if flag[0] == 0 {
        rollback!(
            b"state-foreign: target account's flag is off",
            StateForeignError::FlagOff
        );
    }

    accept!()
}
```

Passing `namespace = None` and `account = &target` reads the entry keyed
`ENABLED_KEY` **in this hook's own namespace, but on `target`'s account** —
the shape for "the same hook code, installed on account A and account B,
where A wants to read a flag B's copy of the hook maintains about itself."
Note this example reads the raw entry directly via `state_foreign` rather
than the typed `state_foreign_get_typed`: the typed layer decodes a
lenient *prefix*, not an exact length, so it would silently tolerate an
oversized `enabled` value this raw code correctly rejects by checking `n ==
flag.len()`. When your value type's exact length matters, decide
deliberately between the raw and typed foreign accessors rather than
reaching for the typed one by default.

## Where to go next

Every typed value type on this page — the `u64` in the counter example, the
`Deposit`/`DepositValue` structs, `AccountId` as a `Balance` key payload —
is either a primitive `rshooks` already implements `ToBytes`/`FromBytes`
for, or a struct built with `#[derive(HookKey)]`/`#[derive(HookData)]`. See
[Typed Data with Derives](typed-data.md) for how those derives work, their
exact byte layout, and why they cost nothing over hand-packing.
