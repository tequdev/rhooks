# xfl-math

## What you'll learn

How to read a transaction's `Amount` as an **XFL** (Xahau's decimal
floating-point type) regardless of whether it's a native (XRP/XAH) or IOU
amount, do a ratio computation (`mulratio`) on it, and compare the result —
handling every step's `Result` explicitly, since every XFL host call is
fallible. Also: hooks-lib's XFL **operator** API end to end — the checked
`Add`/`Sub`/`Mul`/`Div`/`Neg` operators and local `PartialEq`/`PartialOrd`
on plain `XFL`, and `XFLUnchecked`'s poison-propagating hot-path chain.

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
`examples/08_slot-ledger` for more on slot navigation specifically).

### `mulratio` and the operator-based comparison

```rust
let share = amount.mulratio(false, 1, 100)?;   // 1% of `amount`, rounding down
// ...
if share < min_share {
    rollback!(...);
}
```

`mulratio(round_up, num, den)` computes `self * (num / den)` — used here to
take 1% of the transaction amount; it takes two extra scale parameters
beyond `self`/`rhs`, so it stays a named method (no operator shape fits
it). The comparison, though, is now a plain `<` — `hooks_lib::xfl`'s
`PartialOrd` impl on `XFL` is a pure local bit/order comparison (see its
module doc comment for exactly how, and for the sign-magnitude subtlety
that makes it more than a bare `i64` compare of the raw bits), infallible
for canonical values like these, so there is no `Err` arm to write at this
call site at all.

### The checked `Add`/`Sub`/`Mul`/`Div` operators

```rust
let remaining = match amount - share {
    Ok(x) => x,
    Err(_) => rollback!(...),
};
if remaining <= XFL::from_raw_bits(0) {
    rollback!(...);
}
```

`Sub`'s `Output` is `Result<XFL, HookError>` — every XFL arithmetic op is a
fallible host call, so the operators return `Result` rather than panicking
(`hooks_lib::xfl`'s module doc comment covers why, and how `Sub` is built
from `Neg` (local) plus one `float_sum` host call). Handled explicitly here
exactly like every other fallible step in this hook — the operators change
*how* the call is spelled (`amount - share` vs. the pre-operator
`amount.sub(share)`), not whether the `Result` gets checked.
`XFL::from_raw_bits(0)` constructs canonical zero with no host call at all
(the all-zero bit pattern is always valid), and `<=` reuses the same local
`PartialOrd` impl as the minimum-share check above.

### `XFLUnchecked`: a hot-path chain

```rust
let compounded_raw =
    share.unchecked() * growth.unchecked() * growth.unchecked() * growth.unchecked();
let compounded = match compounded_raw.validate() {
    Ok(x) => x,
    Err(_) => rollback!(...),
};
```

`XFLUnchecked` (`hooks_lib::xfl_unchecked`) is the poison-propagating
counterpart to `XFL`: its operators pass the raw `i64` straight into the
next host call with no guest-side `Result` branch in between, then
`validate()` turns the final value into a real `Result<XFL, HookError>`
with one last host round trip. The three-multiply compounding chain here
is purely illustrative — it's nowhere near where per-step `Result`
handling would actually be the measured bottleneck worth optimizing away —
included solely to show the pattern's shape; see
`hooks_lib::xfl_unchecked`'s module doc comment for the full soundness
argument (why a poisoned/invalid operand can never produce a
spuriously-valid result from any of these operators) and its audit table.

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
out of the valid range. The growth-factor constant (`1.01`) later in the
hook is constructed the same way: mantissa `1_010_000_000_000_000`,
exponent `-15`.

## Migrating from the pre-operator method API

`hooks_lib::xfl::XFL` used to expose `mul`/`add`/`div`/`neg`/`eq`/`lt`/
`gt`/`compare` as named methods. All eight are now gone, replaced by
operators (this is the breaking change this example demonstrates
end to end):

| Old method | New spelling |
|---|---|
| `a.mul(b)` | `a * b` (`Output = Result<XFL, HookError>`) |
| `a.add(b)` | `a + b` (`Output = Result<XFL, HookError>`) |
| `a.div(b)` | `a / b` (`Output = Result<XFL, HookError>`) |
| `a.neg()` | `-a` (`Output = XFL` — now infallible; no host call) |
| `a.eq(b)` | `a == b` (`bool`, via `PartialEq` — now infallible; no host call) |
| `a.lt(b)` | `a < b` (`bool`, via `PartialOrd` — now infallible; no host call) |
| `a.gt(b)` | `a > b` (`bool`, via `PartialOrd` — now infallible; no host call) |
| `a.compare(b, mode)` | `a == b` / `a < b` / `a > b` / `a <= b` / `a >= b`, as appropriate |

`a - b` (`Sub`) is new — there was no `sub`/`subtract` method before, since
there's no dedicated `float_subtract` host function; it's built from the
new `Neg` plus `float_sum`. Chains that mix a plain `XFL` with an
already-`Result<XFL, HookError>` value on either side (`a + b + c`) work
without an explicit `?` between steps; see `hooks_lib::xfl`'s module doc
comment for exactly which combinations are (and, for one specific
combination that Rust's orphan rules make impossible, are not) supported.
For hot paths where even that per-step `Result` handling is the measured
cost problem, there's the new `XFLUnchecked` (`hooks_lib::xfl_unchecked`),
demonstrated above.

## Handling XFL's failure modes

Every fallible step here — `otxn_slot`, `slot_subfield`, `XFL::from_slot`,
`mulratio`, `XFL::new`, the `Sub` operator, `XFLUnchecked::validate` — is
matched explicitly and rolls back with a distinct message on `Err`, rather
than being unwrapped. Concretely, the kinds of `HookError` these can
surface include:

| Call | Example failure |
|---|---|
| `slot_subfield` | `DOESNT_EXIST` — no `Amount` field on this transaction type |
| `XFL::from_slot` | `NOT_AN_AMOUNT` — the field isn't an Amount-shaped object |
| `mulratio` | `XFL_OVERFLOW` — the scaled result doesn't fit |
| `XFL::new` | `MANTISSA_OVERSIZED`/`MANTISSA_UNDERSIZED`/`EXPONENT_OVERSIZED`/`EXPONENT_UNDERSIZED` — out-of-range inputs |
| `Sub` (`amount - share`) | `INVALID_FLOAT` — either operand isn't a valid XFL bit pattern |
| `XFLUnchecked::validate` | `INVALID_FLOAT` — the chain's final raw value (or any poisoned value it passed through) didn't validate |

`share < min_share`, `remaining <= XFL::from_raw_bits(0)`, and
`compounded <= share` are all local `PartialOrd` comparisons — infallible,
so none of them has a corresponding failure row above.

## Build

```sh
cargo run -p hooks-build -- build --manifest-path examples/07_xfl-math/Cargo.toml
```

No extra flags needed: every operation here is either a host call
(`otxn_slot`, `slot_subfield`, the `float_*` family) or a scalar operation
(comparison, sign-bit flip) on the resulting `bool`/`i64` values — no
fixed-size array comparison, so no compiler-generated `bcmp`-style loop to
guard.

## Zero-cost check: operators vs. the old method API

Converting the original mulratio-and-compare logic from methods to
operators alone (no new functionality) measures **worst-case
instructions = 155**, strictly *below* the pre-operator method-based
version's 162 — the comparison went from a `float_compare` host call
(`.lt()`) to a local, host-call-free `PartialOrd` comparison (`<`), so
this isn't just "no regression," it's a real reduction. The full version
in this crate — with the `Sub`/`PartialOrd` and `XFLUnchecked` sections
added on top purely for demonstration — measures **269** (see below);
that increase is new functionality being exercised, not operator overhead
on the original logic.

## Expected behavior

- 1% of the transaction `Amount` is at least `0.000001` → accept (subject
  to the `Sub`/`XFLUnchecked` sanity checks below, which should never
  actually trip for a valid positive `Amount`).
- 1% of the transaction `Amount` is below `0.000001` → rollback
  (`"xfl-math: computed share below minimum"`, code `6`).
- Any of the intermediate steps fails (missing `Amount` field, overflow,
  ...) → rollback with that step's specific message and code (see below).

## Error codes

`XflMathError` (`hooks_lib::hook_errors!`, see `src/lib.rs`) is the
`rollback!` code for each failure this hook can exit with:

| variant | code | meaning |
|---|---|---|
| `OtxnSlotFailed` | 1 | `otxn_slot` failed to load the originating transaction into a slot |
| `NoAmountField` | 2 | `slot_subfield` found no `Amount` field on the originating transaction |
| `InvalidAmount` | 3 | `XFL::from_slot` could not decode the `Amount` slot as a valid XFL amount |
| `MulratioFailed` | 4 | `mulratio` failed (e.g. overflow) computing the percentage share |
| `MinShareConstructFailed` | 5 | `XFL::new` failed to construct the fixed minimum-share constant |
| `BelowMinimum` | 6 | the computed share fell below the fixed minimum |
| `RemainingComputeFailed` | 7 | `amount - share` (the checked `Sub` operator) failed |
| `NotEnoughRemaining` | 8 | `amount - share` was not strictly positive |
| `GrowthConstructFailed` | 9 | `XFL::new` failed to construct the fixed growth-factor constant |
| `CompoundValidationFailed` | 10 | the `XFLUnchecked` compounding chain's final `validate()` call failed |
| `CompoundNotIncreasing` | 11 | the compounded value did not come out strictly greater than `share` |
