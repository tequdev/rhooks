# emit-txn

Reserves one emission slot (`etxn_reserve(1)`), builds a minimal Payment
pre-image (`TransactionType`, `Flags`, `Amount` = 1 drop, `Account`,
`Destination` — the sender of the originating transaction), calls
`prepare()` to let the host fill in the remaining system fields
(`Sequence`, `Fee`, `SigningPubKey`, `EmitDetails`, `LastLedgerSequence`),
and `emit()`s the result. Also exports `cbak`, called when the emitted
transaction settles.

See the doc comment at the top of `src/lib.rs` for the exact pre-image
field layout and an important caveat: `prepare()`'s substitution contract
is documented only as "auto-fill system fields" in the reference material
available while writing this example, so this has not been exercised
against a live xahaud node — `hooks-build` validates wasm module shape
only, not wire-level transaction correctness.

## Build

```sh
cargo run -p hooks-build -- build --manifest-path examples/emit-txn/Cargo.toml \
  --auto-guard --default-maxiter 48
```

`--auto-guard` **is** required even though the source has no loop:
zero-initializing the 320-byte `prepared` buffer compiles to a
`compiler_builtins`-style `memset` loop (8 bytes/iteration) under
`opt-level = "z"` on `wasm32v1-none`. `--default-maxiter 48` is used
instead of the `--auto-guard` default of 16 because the loop's true worst
case is about 40 iterations (`320 / 8`) — 16 would build successfully but
risk a runtime `GUARD_VIOLATION` on a real node. See `examples/README.md`'s
"On `--auto-guard`" section for the full story.
