# state-counter

Maintains a persistent counter in Hook state: reads the current 8-byte
little-endian count under a fixed, zero-padded 32-byte state key
(defaulting to zero if absent), increments it, writes it back with
`state_set`, and accepts with the new count as the return-code payload.

The state key is built from a short name (`b"counter"`) with hooks-lib's
`pad!` macro, which zero-pads it to 32 bytes **at compile time** (an inline
`const` block): the padded key is baked into the binary, so no copy loop —
and therefore no loop guard — exists at runtime.

## Build

```sh
cargo run -p hooks-build -- build --manifest-path examples/state-counter/Cargo.toml
```

No extra flags needed — this example is guard-clean without `--auto-guard`.

## Error codes

`StateCounterError` (`hooks_lib::hook_errors!`, see `src/lib.rs`) is the
`rollback!` code for each failure this hook can exit with:

| variant | code | meaning |
|---|---|---|
| `StateSetFailed` | 1 | `state_set` failed to persist the incremented counter |
