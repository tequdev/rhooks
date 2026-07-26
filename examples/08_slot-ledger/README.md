# slot-ledger

## What you'll learn

How to navigate a transaction's fields through the **Slot API**
(`otxn_slot` → `slot_subfield` → `slot`) instead of `otxn_field` directly —
useful once a hook needs to reach into structure `otxn_field` can't address
on its own (arrays, nested objects), and a good warm-up for those cases
even though this example only reads two top-level scalar fields.

## Code walkthrough

```rust
let txn_slot = otxn_slot(0)?;                              // whole otxn → a slot
let dest_slot = slot_subfield(txn_slot, sfDestination, 0)?; // navigate to a field
let dest: AccountId = slot_exact(dest_slot)?;                // serialize that field out
```

(the real code matches on each `Result` individually and rolls back with a
specific message per failure; shown compressed here with `?` for the
walkthrough.)

`otxn_slot(0)` loads the whole originating transaction into a new,
auto-assigned slot (`slot_into = 0` means "auto-assign", used consistently
across the Slot API — see `slot_set`, `slot_subarray`, `slot_subfield`
too). `slot_subfield(parent_slot, field_id, 0)` then extracts one field
from the object in `parent_slot` into its *own* new slot;
`slot_exact(slot_no)` (`hooks_lib::api::slot::slot_exact`) finally
serializes whatever's in a slot into any `hooks_lib::convert::FixedRead`
type — here `AccountId`, inferred from the `let dest: AccountId = ...`
annotation, no turbofish — requiring the result to be exactly that type's
length (`ACC_ID_LEN` for `AccountId`): the same exact-length convention as
`otxn_field_exact`/`state_exact`/`hook_param_exact`, all of which now infer
their return type from context the same way instead of a `::<N>`
turbofish.

This example does that twice from the same `txn_slot`: once for
`sfDestination` (always exactly 20 bytes — an `AccountId`), once for
`sfAmount`. For `Amount`, `slot_size(amount_slot)` is checked *before*
reading anything out: it reports the serialized size (8 bytes for a
native amount, 48 for an IOU one) without copying any data, so the actual
read buffer only ever needs to be sized for the native case this example
supports (rejecting an IOU `Amount` as out of scope, rather than always
allocating room for the larger encoding just to check its length after the
fact). Every one of `otxn_slot`, `slot_subfield`, `slot_size`, and
`slot_exact` returns a `Result`, each handled with its own
[`hooks_lib::hook_errors!`] rollback code and message.

Slots are freed with `slot_clear` once no longer needed — not strictly
required for a single-invocation hook this small, but shown here as the
hygienic default (see the Slot API's "up to 255 slots available" limit).

## Build

```sh
cargo run -p hooks-build -- build --manifest-path examples/08_slot-ledger/Cargo.toml
```

No extra flags needed: every comparison here is a scalar (`usize`)
length check, not a fixed-size array comparison, so there's no
compiler-generated `bcmp`-style loop to guard.

## Expected behavior

- Transaction has no `Destination` field (e.g. not a `Payment`) →
  rollback, code `2`.
- Transaction has a `Destination` but a non-native (IOU) `Amount` →
  rollback (`"unsupported (non-native) Amount"`, code `5`).
- Transaction has both a `Destination` and a native `Amount` → accept, with
  the accept code set to a combination of both fields' first bytes (a
  stand-in for "the values were actually read," not meaningful hook logic
  on its own).

## Error codes

`SlotLedgerError` (`hooks_lib::hook_errors!`, see `src/lib.rs`) is the
`rollback!` code for each failure this hook can exit with:

| variant | code | meaning |
|---|---|---|
| `OtxnSlotFailed` | 1 | `otxn_slot` failed to load the originating transaction into a slot |
| `NoDestinationField` | 2 | `slot_subfield` found no `Destination` field on the originating transaction |
| `UnexpectedDestinationSize` | 3 | `Destination`'s slot didn't serialize to exactly 20 bytes |
| `NoAmountField` | 4 | `slot_subfield` found no `Amount` field on the originating transaction |
| `UnsupportedAmount` | 5 | `Amount` isn't an 8-byte native (XRP/XAH) amount |
| `SlotSizeFailed` | 6 | `slot_size` failed for the `Amount` slot |
| `AmountReadFailed` | 7 | reading `Amount` out of its slot failed after `slot_size` already reported the native-amount length |
