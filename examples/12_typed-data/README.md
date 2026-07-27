# typed-data

## What you'll learn

How to use `#[derive(HookData)]` to treat a **composite, multi-field
struct** as a hook-state key, a hook-state value, and an `otxn_param`/
`hook_param` payload — instead of hand-packing each into a raw byte buffer
yourself — and how to confirm the derive costs nothing extra at the wasm
level (a real worst-case-instruction-count measurement, not just an
assertion).

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
deadline ledger sequence, and a flags byte. Both are declared as ordinary
Rust structs:

```rust
#[derive(HookData, Clone, Copy)]
struct DepositKey {
    tag: u8,
    owner: AccountId,
}

#[derive(HookData, Clone, Copy)]
struct DepositValue {
    amount: u64,
    deadline: u32,
    flags: u8,
}
```

and used directly — no manual byte packing anywhere in `src/lib.rs`:

```rust
let key = DepositKey { tag: DEPOSIT_TAG, owner };
let current = state_get_typed(&key)?.unwrap_or(EMPTY_DEPOSIT);
// ...
state_set_typed_kv(&key, &next)?;
```

## Pairing a key with its value type (and a param name with its type)

`state_get`/`state_set_typed` take the key and the value type as two
*independent* generic parameters — nothing stops calling
`state_get::<SomeOtherValue>(&key)` for a `key`/`SomeOtherValue` combination
that was never meant to go together, as long as `SomeOtherValue: FromBytes`
(true of nearly every fixed-size type, including some *other* key's value
type). The same shape of bug exists for `otxn_param_exact`/`hook_param_exact`:
the parameter name and the value type are independent arguments, so nothing
stops decoding the `INS` parameter as `Config` by mistake.

This crate closes both gaps instead of relying on "just don't mix them up":

```rust
// Ties DepositKey to exactly one value type.
state_key_value!(DepositKey => DepositValue);

// Ties Config/Instruction to exactly one parameter name.
param_name!(Config, CFG_PARAM);
param_name!(Instruction, INS_PARAM);
```

`state_get_typed`/`state_set_typed_kv` (used above) then resolve
`DepositKey`'s value type from `DepositKey` itself — there is no second,
independently-chosen `T` left for a mismatch to hide in — and
`hook_param_typed::<Config>()`/`otxn_param_typed::<Instruction>()` resolve
their parameter name from the type itself, with **no name argument at the
call site at all**. Passing the wrong value type for `DepositKey` (e.g.
`state_set_typed_kv(&key, &some_other_struct)`) is now a compile error, not
a silent bug waiting to be discovered on a live node — see
`hooks_lib::state::TypedStateKey`'s and `hooks_lib::convert::ParamName`'s
doc comments for the full rationale, and `hooks_lib::HookData`'s doc
comment for a `compile_fail` example pinning the mismatch case. Both
wrappers are `#[inline(always)]` pass-throughs to the loose functions, so
this costs nothing: this crate's worst-case instruction count is identical
whether written with the loose or the paired API (413, either way).

A Hook API parameter name isn't always a plain tag like `"CFG"`/`"INS"`,
either — per the Hook API itself, it's a genuine variable-length key of up
to 32 bytes, and (exactly like a hook state key) can be a whole composite,
struct-shaped value instead of a literal byte string. `ParamName` (used
above) only covers the plain-byte-string case; a **composite** parameter
name uses the separate `ParamKey` trait/`param_key!` macro and
`hook_param_typed_kv`/`otxn_param_typed_kv` instead — kept as a distinct,
opt-in path specifically so this crate's `Config`/`Instruction` (both
plain byte-tag names) never pay for it: encoding an arbitrary struct into
bytes at runtime is a small but real cost (Rust has no stable way to run a
trait method like `ToBytes::write` at compile time), whereas a plain
byte-string name is already-encoded static data with none. See
`hooks_lib::convert::ParamKey`'s doc comment for a composite-name worked
example.

## Before/after: what `#[derive(HookData)]` replaces

Without it, `DepositKey`/`DepositValue` would have to be hand-packed into
raw byte buffers — the way every hook (including this crate's `Config`/
`Instruction`, if this feature didn't exist) had to before this feature:

```rust
// Key: tag (1 byte) || owner (20 bytes) || zero pad, 32 bytes total.
fn make_key(owner: &AccountId) -> [u8; 32] {
    let mut out = [0u8; 32];
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
the root README's "`--auto-guard`" section). `#[derive(HookData)]` avoids
that by construction (every offset is a compile-time constant, every
per-field copy an inlined `ToBytes::write`/`FromBytes::read` call — see its
doc comment's "Zero-cost by construction" section) — but the only way to
*prove* that is to build both versions through `hooks-build` and compare
`hooks-build check`'s reported worst-case instruction count.

This table is a real `hooks-build build`/`check` measurement of this exact
hook's logic, built twice — once as committed (`#[derive(HookData)]`), once
with `DepositKey`/`DepositValue`/`Config`/`Instruction` replaced by the
hand-packed functions above (everything else byte-for-byte identical):

| version | worst-case instructions | wasm size |
|---|---|---|
| `#[derive(HookData)]` (this crate, as committed) | 413 | 1433 bytes |
| hand-packed (`.get()`/`.get_mut()` per field, as most hooks write it today) | 545 | 1708 bytes |

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

Both `CFG` (installed at `SetHook` time) and `INS` (attached to each
`Invoke` transaction) are `#[derive(HookData)]` structs, so their wire
layout is exactly "every field, in declaration order, little-endian,
back-to-back" (`hooks_lib::convert`'s crate-wide convention — see
`Config`/`Instruction`'s generated `LEN` rustdoc for the field table).

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
