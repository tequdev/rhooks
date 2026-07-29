# state-counter

Maintains a persistent counter in Hook state: reads the current 8-byte
little-endian count under a short, literal state key (defaulting to zero
if absent), increments it, writes it back with `state_set`, and accepts
with the new count as the return-code payload.

The state key is just `b"counter"` (7 bytes), sent to the host exactly
as-is — the same idiom as the C hook `state(&v, 8, "counter", 7)`. The Hook
API itself accepts any key from 1 to 32 bytes and left-pads a shorter one
internally, so there is no need to build a full, locally zero-padded
32-byte key by hand (see `hooks_lib::state`'s module doc comment, "Key
length and padding," for the full rule).

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
