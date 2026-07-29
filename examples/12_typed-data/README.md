# typed-data

## What you'll learn

How to declare a composite hook-state key/value pair and a composite Hook
API parameter name/value pair in **one line each**, via `hook_state!`'s/
`hook_parameter!`'s/`otxn_parameter!`'s declaration-macro grammar — instead
of hand-packing each into a raw byte buffer yourself — and how to confirm
the generated code costs nothing extra at the wasm level (a real
worst-case-instruction-count measurement, not just an assertion).

Under the hood, each declaration still expands to the same four narrow,
purpose-built impls this example used to spell out by hand with separate
derives — `#[derive(HookKey)]`, `#[derive(HookData)]`,
`#[derive(ParamName)]`, `#[derive(ParamValue)]` — plus the pairing trait:

| role | generates | example (declared inline below) |
|---|---|---|
| hook-state **key** | `ToBytes` + `StateKeyEncode` (≤32-byte check) | `DepositKey` |
| hook-state **value** | `ToBytes` + `FromBytes` + `FixedRead` + `LEN` | `DepositValue` |
| Hook API parameter **name** | `ToBytes` (1–32-byte check) | `AdminName` |
| Hook API parameter **value** | `FromBytes` + `FixedRead` | `Config`, `Instruction`, `PauseSwitch` |

A key/name is only ever *encoded outward* (to locate something); a
value/payload is only ever *decoded* (read back) — that read/write split is
exactly why these are four separate roles rather than one covering
everything. See `hooks_lib::hook_state!`'s doc comment for the full
declaration-macro grammar (five forms, from a fully-fixed key/name down to
a fully composite, runtime-constructed one), and each underlying derive's
own rustdoc (`hooks_lib::{HookKey, HookData, ParamName, ParamValue}`) for
the codegen rationale and `compile_fail` examples pinning misuse.

## The hook

A per-account deposit ledger, invoked via an `Invoke` transaction. Every
invocation attaches its own instruction as a Hook parameter on the
transaction itself (`INS`, read via `otxn_param`) — distinct from the
hook's own installed configuration (`CFG`, read via `hook_param`, the same
mechanism `examples/03_hook-params` uses for a single value, here extended
to a whole struct):

- `deposit` (`action = 1`): rejects (rolls back) if the deposited amount is
  below the configured minimum; otherwise adds it to the sender's balance
  and (re)starts a lock window ending `lock_ledgers` ledgers from now.
- `withdraw` (`action = 2`): rejects if the sender has no outstanding
  deposit, or if the lock window hasn't elapsed yet; otherwise zeroes the
  balance out.

Each sender's record is looked up by a **composite key** — a tag byte plus
their `AccountId` — and stored as a **composite value** — an amount, a
deadline ledger sequence, and a flags byte. One `hook_state!` declaration
(**Form 3**: a struct-shaped key, constructed at each call site, with an
**inline** value definition) covers both:

```rust
hook_state!(DepositKey {tag: u8, owner: AccountId} => DepositValue {amount: u64, deadline: u32, flags: u8});
```

— equivalent to separately declaring `#[derive(HookKey)] struct
DepositKey { tag: u8, owner: AccountId }`, `#[derive(HookData)] struct
DepositValue { amount: u64, deadline: u32, flags: u8 }`, and
`hook_state!(DepositKey => DepositValue)` to pair them (see
`hooks_lib::hook_state!`'s doc comment for that longhand backward-compatible
form) — and used directly — no manual byte packing anywhere in
`src/lib.rs`:

```rust
let key = DepositKey { tag: DEPOSIT_TAG, owner };
let current = state_get_typed(&key)?.unwrap_or(EMPTY_DEPOSIT);
// ...
state_set_typed(&key, &next)?;
```

## Pairing a key with its value type (and a param name with its value type)

`state_get`/`state_set_loose` take the key and the value type as two
*independent* generic parameters — nothing stops calling
`state_get::<SomeOtherValue>(&key)` for a `key`/`SomeOtherValue` combination
that was never meant to go together, as long as `SomeOtherValue: FromBytes`
(true of nearly every fixed-size type, including some *other* key's value
type). The same shape of bug existed for `otxn_param_exact`/`hook_param_exact`:
the parameter name and the value type are independent arguments, so nothing
stops decoding the `INS` parameter as `Config` by mistake.

This crate closes both gaps the same way — a one-line pairing declaration,
then an accessor that takes **a reference to a key/name value** and
resolves the paired type from it, never a turbofish or an independently
inferred return type:

```rust
// Ties DepositKey to exactly one value type (Form 3, shown above already
// declares this pairing — repeated here only to name it explicitly).
hook_state!(DepositKey {tag: u8, owner: AccountId} => DepositValue {amount: u64, deadline: u32, flags: u8});

// Ties CfgName/InsName to exactly one parameter value type each —
// hook_parameter! for a hook's own installed parameter, otxn_parameter!
// for one attached to the originating transaction (same grammar, same
// TypedParamName impl). Form 1: `Name = bytes => Ty` declares `Name` as a
// new zero-sized type *and* ties it to the fixed byte-string name and the
// value type, all in one line — no separate `struct CfgName;` needed.
hook_parameter!(CfgName = b"CFG" => Config {min_amount: u64, lock_ledgers: u32});
otxn_parameter!(InsName = b"INS" => Instruction {action: u8, amount: u64});
```

`state_get_typed(&key)`/`state_set_typed(&key, &value)` (used above) resolve
`DepositKey`'s value type from the `key` argument itself — there is no
second, independently-chosen `T` left for a mismatch to hide in — and
`hook_param_typed(&CfgName)`/`otxn_param_typed(&InsName)` resolve `Config`/
`Instruction` from the *name argument*, the same way. `Config`/
`Instruction`/`DepositValue` never need a type annotation anywhere in
`src/lib.rs` (see `config()`/`my_hook()`) — the argument alone always picks
the right type. Passing the wrong value type for `DepositKey` (e.g.
`state_set_typed(&key, &some_other_struct)`) is now a compile error, not
a silent bug waiting to be discovered on a live node — see
`hooks_lib::state::TypedStateKey`'s and `hooks_lib::convert::TypedParamName`'s
doc comments for the full rationale, and `hooks_lib::HookKey`'s doc
comment for a `compile_fail` example pinning the mismatch case.
`state_get_typed`/`state_set_typed` and `hook_param_typed`/`otxn_param_typed` cost
nothing beyond the loose functions they replace, *for a plain-tag
parameter name* — measured at 441 worst-case instructions either way, the
same as this hook's logic minus the `AdminName` pause switch covered next
(see that section for the one place this hook's cost *does* go up, and
why).

A Hook API parameter name isn't always a plain tag like `"CFG"`/`"INS"`,
either — per the Hook API itself, it's a genuine variable-length key of up
to 32 bytes, and (exactly like a hook state key) can be a whole composite,
struct-shaped value instead of a literal byte string. `hook_parameter!`/
`otxn_parameter!` cover both, via the same declaration-macro grammar
`hook_state!` uses (see `hooks_lib::hook_state!`'s doc comment for the full
staircase) — Form 1 for a fully fixed name (`CfgName`/`InsName` above),
Form 3 for a struct-shaped one constructed per call site (`AdminName`
below) — and both are read by the exact same `hook_param_typed`/
`otxn_param_typed`. (`hook_parameter!`/`otxn_parameter!` are two separate
macros with identical grammar and expansion — purely so the declaration
site documents which of `hook_param`/`otxn_param` a name is meant for; see
`hooks_lib::convert::TypedParamName`'s doc comment.) Only a *plain,
already-known-at-compile-time* name is free, though — Form 1 (used above)
overrides `TypedParamName::name_bytes` to hand over the already-`'static`
literal bytes directly, at zero runtime cost, while Form 3 (used below,
for `AdminName`) relies on `TypedParamName::name_bytes`'s default body, a
small, genuine runtime encode via `Name`'s own `ToBytes` impl, unavoidable
for an arbitrary composite type — Rust has no stable way to run a trait
method at compile time. `hook_parameter!`/`otxn_parameter!` also keep the
*original*, comma-separated 3-argument legacy form
(`hook_parameter!(Name, bytes => Ty)`, `Name` declared separately by the
caller) for backward compatibility — see `hooks_lib::hook_parameter!`'s doc
comment for a worked example. See `hooks_lib::convert::TypedParamName`'s
doc comment for the full zero-cost rationale, and the "Composite parameter
names" section below for this hook's own worked composite-name example and
its measured cost.

## Before/after: what `#[derive(HookKey)]`/`#[derive(HookData)]` replace

Without them, `DepositKey`/`DepositValue` would have to be hand-packed into
raw byte buffers — the way every hook (including this crate's `Config`/
`Instruction`, if this feature didn't exist) had to before this feature:

```rust
// Key: tag (1 byte) || owner (20 bytes) - 21 bytes total, sent to the
// host exactly as-is: the host itself left-pads a key shorter than its
// fixed 32-byte storage width (see `hooks_lib::state`'s module doc
// comment, "Key length and padding") - no local zero-padding here.
fn make_key(owner: &AccountId) -> [u8; 21] {
    let mut out = [0u8; 21];
    if let Some(b) = out.get_mut(0) {
        *b = DEPOSIT_TAG;
    }
    if let Some(dst) = out.get_mut(1..21) {
        dst.copy_from_slice(owner.as_ref());
    }
    out
}

// Value: amount (8 bytes LE) || deadline (4 bytes LE) || flags (1 byte).
fn encode_value(v: &DepositValue) -> [u8; 13] {
    let mut out = [0u8; 13];
    if let Some(dst) = out.get_mut(0..8) {
        dst.copy_from_slice(&v.amount.to_le_bytes());
    }
    if let Some(dst) = out.get_mut(8..12) {
        dst.copy_from_slice(&v.deadline.to_le_bytes());
    }
    if let Some(b) = out.get_mut(12) {
        *b = v.flags;
    }
    out
}

fn decode_value(buf: &[u8; 13]) -> Option<DepositValue> {
    let amount = buf.get(0..8)?;
    let deadline = buf.get(8..12)?;
    let flags = *buf.get(12)?;
    // ... four more lines assembling `u64`/`u32` from LE bytes ...
}
```

— every field's offset counted by hand, every reader kept in sync with
every writer by hand, and every one of those `.get()`/`.get_mut()` calls
(required by this crate's `indexing_slicing` deny — see `docs/DESIGN.md`
§2/§8) repeated per field. `#[derive(HookData)]` generates the equivalent
of the above (the same fixed, compile-time offsets, the same
`.get_mut()`-guarded fixed-size copies) once, from the struct definition
itself, and keeps `ToBytes`/`FromBytes` in sync automatically as fields are
added, removed, or reordered.

## Zero-cost: measured, not assumed

`docs/DESIGN.md` and this crate's own doc comments repeatedly warn that a
"clean-looking" abstraction can silently introduce an unguarded loop
(`memcpy`/`memset`/`bcmp` lowering — see `examples/06_guard-patterns` and
the root README's "`--auto-guard`" section). `#[derive(HookKey)]`/
`#[derive(HookData)]`/`#[derive(ParamValue)]` avoid that by construction
(every offset is a compile-time constant, every per-field copy an inlined
`ToBytes::write`/`FromBytes::read` call — see `HookData`'s doc comment's
"Zero-cost by construction" section, which all three derives share) — but
the only way to *prove* that is to build both versions through
`hooks-build` and compare `hooks-build check`'s reported worst-case
instruction count.

This table is a real `hooks-build build`/`check` measurement, at this
workspace's `opt-level = 3` default (`examples/Cargo.toml`, `docs/
DESIGN.md`'s §2 C6), of this hook's core deposit-ledger logic (the state
key/value pairing plus the plain-tag `CFG`/`INS` parameters; not yet
counting the `AdminName` composite parameter name covered below), built
twice — once with the derives (`DepositKey`/`DepositValue`/`Config`/
`Instruction`, as committed), once with all four replaced by the
hand-packed functions above (everything else byte-for-byte identical):

| version | worst-case instructions | wasm size |
|---|---|---|
| derived (this crate, as committed) | 441 | 1504 bytes |
| hand-packed (`.get()`/`.get_mut()` per field, as most hooks write it today) | 525 | 1674 bytes |

> **Note:** this table predates `DepositKey`'s `HookKey`-derived
> `StateKeyEncode::encode()` sending its own real length (21 bytes) instead
> of a locally zero-padded 32 (see `docs/DESIGN.md` §5.7) — both rows would
> shift down slightly if re-measured today; the relative comparison
> (derived cheaper than hand-packed) is unaffected. The full committed
> hook's current, directly measured numbers (including `AdminName`) are
> 483 instructions / 1642 bytes — see the note after the next table.

The derive isn't just *as cheap as* hand-packing here — it measures
**cheaper**: the generated `write`/`read` check the struct's total length
**once** (`buf.get_mut(..Self::MAX_LEN)`), then copy every field through
already-proven-in-bounds fixed offsets, whereas the naive hand-packed
version above re-checks bounds with a separate `.get()`/`.get_mut()` call
per field — extra branches the derive's single up-front check avoids. (A
hand-written version that also front-loads one length check could match
the derive's number; the point isn't that hand-packing can't be made this
cheap, it's that the derive *always* generates that shape, by construction,
without a hook author having to discover and apply the trick themselves.)

No `--auto-guard`/`--default-maxiter` flags are needed for either version —
`hooks-build check` reports both as guard-clean at the source level (see
`examples/README.md`'s "On `--auto-guard`" section for what that means and
why it's the idiom this crate prefers).

## Hook parameter hex encoding

Both `CFG` (installed at `SetHook` time, named by `CfgName`) and `INS`
(attached to each `Invoke` transaction, named by `InsName`) decode as
`#[derive(ParamValue)]` structs, so their wire layout is exactly "every
field, in declaration order, little-endian, back-to-back"
(`hooks_lib::convert`'s crate-wide convention — see `Config`/
`Instruction`'s generated field-layout rustdoc).

`Config { min_amount: u64, lock_ledgers: u32 }` — **12 bytes**. For
`min_amount = 5,000,000` drops (5 XAH) and `lock_ledgers = 20`:

```
min_amount  (u64 LE): 40 4B 4C 00 00 00 00 00
lock_ledgers(u32 LE): 14 00 00 00
CFG value hex:        404B4C000000000014000000
```

```json
{
  "HookParameter": {
    "HookParameterName": "434647",
    "HookParameterValue": "404B4C000000000014000000"
  }
}
```

(`HookParameterName` is `CFG` in ASCII hex.) Omitting `CFG` entirely falls
back to the compiled-in default (1 XAH minimum, a 10-ledger lock).

`Instruction { action: u8, amount: u64 }` — **9 bytes**. For a `deposit` of
6,000,000 drops (6 XAH):

```
action        (u8):    01
amount   (u64 LE): 80 8D 5B 00 00 00 00 00
INS value hex:     01808D5B0000000000
```

Attached directly to the `Invoke` transaction's own `HookParameters` array
(not the `SetHook`'s):

```json
{
  "TransactionType": "Invoke",
  "Account": "...",
  "Destination": "...",
  "HookParameters": [
    {
      "HookParameter": {
        "HookParameterName": "494E53",
        "HookParameterValue": "01808D5B0000000000"
      }
    }
  ]
}
```

A `withdraw` needs no meaningful `amount` (it always empties the whole
balance) but the field still has to be present — 9 bytes total, e.g.
`02` + 8 zero bytes = `020000000000000000`.

## Composite (struct-shaped) parameter names: `AdminName`/`PauseSwitch`

`CFG` and `INS` above are both plain byte-string tags — the common case,
but per the Hook API itself a parameter name is really a variable-length
key of up to 32 bytes, and (exactly like a hook state key) can be a whole
composite, struct-shaped value instead of a literal string. This hook's
operator-controlled pause switch is named that way:

```rust
hook_parameter!(AdminName {section: u8, field: u8} => PauseSwitch {paused: u8});

const ADMIN_PAUSE: AdminName = AdminName { section: 0, field: 0 };
```

This is `hook_parameter!`'s **Form 3** — a struct-shaped name, constructed
per call site — with an inline `PauseSwitch` value, the same shape
`DepositKey`/`DepositValue` used above. Under the hood, `AdminName` still
gets `ParamName`-equivalent codegen (`ToBytes` only — no `FromBytes`, no
`FixedRead`, no inherent `LEN` const), never `HookData`-equivalent codegen:
a Hook parameter *name* is a genuinely different concept from a hook-state
key/value or a parameter *payload* (`PauseSwitch`, which — being something
this hook actually reads back and decodes — gets `ParamValue`-equivalent
codegen instead, same as `Config`/`Instruction`): a name is only ever
**written**, to locate a value, never read back and decoded as itself —
see `hooks_lib::ParamName`'s doc comment for the full rationale, and its
`compile_fail` examples pinning that a `ParamName`-shaped type can't be
read back as a value. Because `AdminName` is Form 3 (not Form 1, the fully
fixed form `CfgName`/`InsName` above use), the generated `TypedParamName`
impl relies on `name_bytes`'s default (genuine-encode) body instead of the
zero-copy override — see the "Measured cost of a composite name" section
below for what that costs. The `const ADMIN_PAUSE` declared separately
above (not part of `hook_state!`'s Form 2 fixed-instance mechanism, since
this name scheme is meant to accommodate *multiple* future
administrative parameters, not just one canonical instance) is what
`hook_param_typed` takes **a reference to** in [`deposits_paused`],
`PauseSwitch`'s type inferred from that argument, no annotation.

### The 1–32-byte constraint

A Hook API parameter name must be **1 to 32 bytes** (`hook_api.h`:
`TOO_SMALL` below 1, `TOO_BIG` above 32 — see
`hooks_lib::convert::PARAM_NAME_MAX_LEN`). `AdminName` encodes to
`section` (1 byte) + `field` (1 byte) = **2 bytes**, comfortably inside
that range. Unlike an oversized `HookData` struct (which has no such bound
at all — a state *value* has no fixed size cap), `#[derive(ParamName)]`
checks this bound **unconditionally, at the struct's own definition** — a
`ParamName` struct that encoded to, say, 40 bytes would fail to compile
right there, before anything tried to use it as a parameter name at all
(the same derive-time-check idea `#[derive(HookKey)]` applies to a
33+-byte state key, just with an added *lower* bound a key doesn't have).
See `hooks_lib::ParamName`'s doc comment for the `compile_fail` example
pinning exactly that case.

### Hex encoding

`AdminName { section: 0, field: 0 }` — **2 bytes** (`section`, then
`field`, no padding — `hooks_lib::convert`'s crate-wide "every field, in
declaration order, back-to-back" convention, same as `Config`/
`Instruction` above): `0000`.

`PauseSwitch { paused: 1 }` — **1 byte**: `01`.

Installed at `SetHook` time (an administrative control, not something a
depositor sets per transaction — hence `hook_param`, not `otxn_param`):

```json
{
  "HookParameter": {
    "HookParameterName": "0000",
    "HookParameterValue": "01"
  }
}
```

Omitting this `HookParameter` entirely (or setting `HookParameterValue`
to `00`) leaves deposits unpaused — `deposits_paused()` treats "absent, or
the wrong size" the same as `paused == 0`.

### Measured cost of a composite name

Unlike the plain `CFG`/`INS` tags (measured identical to the loose API in
the "Pairing a key with its value type" section above), a **composite**
parameter name is not free: `TypedParamName::name_bytes`'s default body has
to actually run `AdminName::write(..)` at runtime (Rust has no stable way to
run a trait method at compile time, so this can't be folded away for an
arbitrary `ToBytes` type — see `hooks_lib::convert::TypedParamName`'s doc
comment). Measured by building this exact hook twice — once as committed
(with the `AdminName`/`PauseSwitch` pause switch), once with that whole
feature removed (everything else byte-for-byte identical):

| version | worst-case instructions | wasm size |
|---|---|---|
| without the `AdminName` pause switch | 441 | 1504 bytes |
| with the `AdminName` pause switch (as committed, pre-`HookKey`-real-length change) | 489 | 1652 bytes |
| with the `AdminName` pause switch (as committed, current) | 483 | 1642 bytes |

+48 instructions, +148 bytes over the no-`AdminName` baseline at the time
this table was measured — the honest, unavoidable cost of one
composite-name-keyed `hook_param` lookup (the struct encode itself, plus
the extra branch/rollback path checking it). Still guard-clean at the
source level: no `--auto-guard`/`--default-maxiter` needed either way.
`DepositKey`'s `HookKey` derive now sends `StateKeyEncode::encode()`'s real
21-byte length instead of a locally zero-padded 32 (`docs/DESIGN.md` §5.7),
shaving 6 instructions/10 bytes off the "as committed" row (the
no-`AdminName` baseline above predates that change too and wasn't
re-measured, but would shift by roughly the same amount).

## Build

```sh
cargo run -p hooks-build -- build --manifest-path examples/12_typed-data/Cargo.toml
```

No extra flags — see "Zero-cost: measured, not assumed" above.

## Expected behavior

- No `INS` parameter (or the wrong size) on the `Invoke` → rollback
  (`"typed-data: INS parameter missing or malformed"`, code `2`).
- `deposit` below the configured (or default) minimum → rollback
  (`"typed-data: deposit below configured minimum"`, code `4`).
- `deposit` at or above the minimum → accept; the account's stored
  `DepositValue.amount` increases by the deposited amount and the lock
  window resets.
- `withdraw` with no outstanding deposit → rollback
  (`"typed-data: nothing to withdraw"`, code `5`).
- `withdraw` before the lock window elapses → rollback
  (`"typed-data: deposit still locked"`, code `6`).
- `withdraw` after the lock window elapses → accept; the account's stored
  `DepositValue` is zeroed out.
- `action` anything other than `1`/`2` → rollback
  (`"typed-data: unknown INS action"`, code `3`).

## Error codes

`TypedDataError` (`hooks_lib::hook_errors!`, see `src/lib.rs`) is the
`rollback!` code for each failure this hook can exit with:

| variant | code | meaning |
|---|---|---|
| `AccountFieldMissing` | 1 | the originating transaction has no `sfAccount` field (unreachable in practice) |
| `InstructionMissing` | 2 | the `INS` Hook parameter is missing, or not exactly 9 bytes |
| `UnknownAction` | 3 | `Instruction::action` is neither `1` (deposit) nor `2` (withdraw) |
| `BelowMinimum` | 4 | a `deposit` instruction's amount fell below the configured minimum |
| `NothingToWithdraw` | 5 | a `withdraw` instruction, but the account has no outstanding deposit |
| `StillLocked` | 6 | a `withdraw` instruction, but the deposit's lock window hasn't elapsed yet |
| `StateReadFailed` | 7 | reading the account's `DepositValue` failed with something other than "no entry" |
| `StateSetFailed` | 8 | writing the updated `DepositValue` back failed |
