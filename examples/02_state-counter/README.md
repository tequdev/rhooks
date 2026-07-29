# state-counter

Maintains a persistent counter in Hook state: reads the current `u64`
count (defaulting to zero if absent or of unexpected size), increments it,
writes it back, and accepts with the new count as the return-code
payload.

This is the minimal tutorial for `hooks_lib`'s **typed storage layer**
(`crate::state`'s `state_get_typed`/`state_set_typed`, paired via
`hook_state!`'s **Form 2** — a struct key with a fixed instance) — no
hand-rolled `[0u8; 8]` buffer, no manual `from_le_bytes`/`to_le_bytes`, no
length check, and (unlike the equivalent hand-written
`#[derive(HookKey)]` + `hook_state!(Key => Value)` + a separate `const`)
no repetition of the key's name three times over:

```rust
hook_state!(CounterKey {name: [u8; 7]} = {name: *b"counter"} => u64);

let count = state_get_typed(&CounterKey).unwrap_or(None).unwrap_or(0);
let next = count.wrapping_add(1);
state_set_typed(&CounterKey, &next);
```

`hook_state!`'s Form 2 declares, from that single line: the `CounterKey`
struct, its `HookKey`-equivalent `ToBytes`/`StateKeyEncode` codegen, a
`const CounterKey: CounterKey = CounterKey { .. };` holding the one fixed
instance (legal because a type name and a value name live in separate
namespaces — `&CounterKey` at the call site refers to the `const`), and
the `TypedStateKey` pairing with `u64`. See `hooks_lib::hook_state!`'s doc
comment for the full grammar staircase this is one step of.

## Why a struct key, not a bare `[u8; 7]` key

`CounterKey { name: *b"counter" }` encodes to exactly the same 7 bytes a
bare `*b"counter"` array key would — see "Same slot as before" below — so
this isn't about the bytes on the wire. It's required by Rust's **orphan
rule**: `hook_state!`'s expansion includes `impl TypedStateKey for
CounterKey`, and implementing a `hooks_lib` trait for a bare `[u8; 7]` (a
`core` type, foreign to this hook crate) from outside `hooks_lib` itself is
not allowed — only implementing it for a type *this crate defines* is. See
`hooks_lib::hook_state!`'s doc comment for the full explanation and a
`compile_fail` example of the bare-array case.

## Same slot as before: real-length encoding, host left-pads

`CounterKey`'s only field is a plain `[u8; 7]`, and `hook_state!`'s Form 2
sends a struct at its own real encoded length (7 bytes here — see
`hooks_lib::state`'s module doc comment, "Key length and padding," and
`docs/DESIGN.md` §5.7) — never locally zero-padded up to the fixed 32-byte
key space. That is exactly the same 7 bytes a bare `*b"counter"` array key
sends (this example's original form, before switching to the typed
layer), so `CounterKey { name: *b"counter" }` lands on the identical,
host-left-padded on-ledger slot — the same idiom as the C hook
`state(&v, 8, "counter", 7)`.

## Build

```sh
cargo run -p hooks-build -- build --manifest-path examples/02_state-counter/Cargo.toml
```

No extra flags needed — this example is guard-clean without `--auto-guard`.

## Error codes

`StateCounterError` (`hooks_lib::hook_errors!`, see `src/lib.rs`) is the
`rollback!` code for each failure this hook can exit with:

| variant | code | meaning |
|---|---|---|
| `StateSetFailed` | 1 | `state_set` failed to persist the incremented counter |

## Cost of the typed layer, here

The typed layer's convenience (no hand-written buffer/length-check/
byte-order code) isn't free: `state_get_typed`/`state_set_typed` go
through `crate::state`'s generic, 32-byte-scratch-buffer machinery
(`MAX_TYPED_STATE_LEN`), rather than this hook reading/writing a plain
8-byte buffer via the raw `state`/`state_set` calls directly. Measured
(`hooks-build build`/`check`): 254 worst-case instructions / 740 bytes,
versus 58 / 349 for the previous, hand-rolled-buffer version of this same
hook. Still guard-clean at the source level — no `--auto-guard`/
`--default-maxiter` needed. For a hook this simple (one `u64` counter,
one key), the raw layer is the cheaper choice; this example uses the
typed layer anyway because its purpose is to be the smallest possible
tutorial for it — see `examples/12_typed-data` for the typed layer's
actual selling point (a *composite*, multi-field key/value pair, where
hand-packing would be far more error-prone than the cost shown here).
