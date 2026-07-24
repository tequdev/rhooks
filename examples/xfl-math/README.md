# xfl-math

## What you'll learn

How to read a transaction's `Amount` as an **XFL** (Xahau's decimal
floating-point type) regardless of whether it's a native (XRP/XAH) or IOU
amount, do a ratio computation (`mulratio`) on it, and compare the result —
handling every step's `Result` explicitly, since every XFL operation is a
fallible host call.

## Code walkthrough

### Reading the Amount as XFL, via a slot

```rust
let txn_slot = otxn_slot(0)?;                       // load the otxn into a slot
let amount_slot = slot_subfield(txn_slot, sfAmount, 0)?;  // navigate to Amount
let amount = XFL::from_slot(amount_slot)?;           // decode as XFL
```

(the real code above matches on each `Result` and rolls back with a
specific message per failure, rather than using `?` — shown here
compressed for the walkthrough).

`otxn_slot(0)` loads the originating transaction into a new, auto-assigned
slot (`0` for `slot_into` means "auto-assign", the same convention used
throughout the Slot API). `slot_subfield(parent, field_id, 0)` then
extracts one field's own slot from it. `XFL::from_slot` (`slot_float` under
the hood) reads whatever's in that slot as an XFL — **this works
identically for an 8-byte native amount or a 48-byte IOU amount**, which is
the main advantage over parsing the raw bytes by hand (compare
`hook-params`/`errors`, which only understand the native case and reject
IOU amounts outright).

An equivalent, non-slot route to the same `XFL` exists too (see the
in-source comment): read the raw Amount bytes with `otxn_field`, then
`XFL::sto_set(&buf[..n])`. Either works; this example uses the slot route
because it's also a chance to show `otxn_slot`/`slot_subfield` (see
`examples/slot-ledger` for more on slot navigation specifically).

### `mulratio` and the `Result`-based comparisons

```rust
let share = amount.mulratio(false, 1, 100)?;   // 1% of `amount`, rounding down
// ...
match share.lt(min_share) {
    Ok(true) => rollback!(...),
    Ok(false) => {}
    Err(_) => rollback!(...),
}
```

`mulratio(round_up, num, den)` computes `self * (num / den)` — used here to
take 1% of the transaction amount. `XFL` deliberately has **no**
`PartialOrd`/`core::ops::*` impls (see `hooks_lib::xfl`'s module doc
comment): both arithmetic and comparison are host calls that can fail (an
overflowed mantissa/exponent, a comparison against an invalid float,
...), and a type that silently panicked or silently returned `false` on
error would be unacceptable for financial logic. `.lt()`/`.gt()`/`.eq()`
all return `Result<bool>`, with three-way handling: `Ok(true)`, `Ok(false)`,
and `Err(_)` are all matched explicitly.

### Constructing a fixed XFL constant

```rust
let min_share = XFL::new(-21, 1_000_000_000_000_000)?;
```

XFL's mantissa is normalized to 16 significant digits (`10^15` to
`10^16 - 1`, per `hooks_lib::xfl`'s module doc comment on the bit layout),
so `0.000001` (1e-6) is written as mantissa `1_000_000_000_000_000` (1e15)
with exponent `-21` (`1e15 * 10^-21 == 10^-6`) — not exponent `-6`, which
with that mantissa would be `1e9`. Getting this wrong is an easy mistake;
`XFL::new` returning `Result` (rather than silently normalizing or
truncating) is what surfaces it if the exponent/mantissa combination is
out of the valid range.

## Handling XFL's failure modes

Every fallible step here — `otxn_slot`, `slot_subfield`, `XFL::from_slot`,
`mulratio`, `XFL::new`, `.lt()` — is matched explicitly and rolls back with
a distinct message on `Err`, rather than being unwrapped. Concretely, the
kinds of `HookError` these can surface include:

| Call | Example failure |
|---|---|
| `slot_subfield` | `DOESNT_EXIST` — no `Amount` field on this transaction type |
| `XFL::from_slot` | `NOT_AN_AMOUNT` — the field isn't an Amount-shaped object |
| `mulratio` | `XFL_OVERFLOW` — the scaled result doesn't fit |
| `XFL::new` | `MANTISSA_OVERSIZED`/`MANTISSA_UNDERSIZED`/`EXPONENT_OVERSIZED`/`EXPONENT_UNDERSIZED` — out-of-range inputs |
| `.lt()` | `INVALID_FLOAT` — either operand isn't a valid XFL bit pattern |

## Build

```sh
cargo run -p hooks-build -- build --manifest-path examples/xfl-math/Cargo.toml
```

No extra flags needed: every operation here is either a host call
(`otxn_slot`, `slot_subfield`, the `float_*` family) or a scalar comparison
on the resulting `bool`/`i64` values — no fixed-size array comparison, so
no compiler-generated `bcmp`-style loop to guard.

## Expected behavior

- 1% of the transaction `Amount` is at least `0.000001` → accept.
- 1% of the transaction `Amount` is below `0.000001` → rollback
  (`"xfl-math: computed share below minimum"`).
- Any of the intermediate steps fails (missing `Amount` field, overflow,
  ...) → rollback with that step's specific message.
