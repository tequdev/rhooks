# state-counter

Maintains a persistent counter in Hook state: reads the current 8-byte
little-endian count under a fixed, zero-padded 32-byte state key
(defaulting to zero if absent), increments it, writes it back with
`state_set`, and accepts with the new count as the return-code payload.

The state key is built by hand from a short name (`b"counter"`) via a
bounded loop carrying `guard!` — the one loop in this example's source, and
the only one needed: it builds clean with no extra flags.

## Build

```sh
cargo run -p hooks-build -- build --manifest-path examples/state-counter/Cargo.toml
```

No extra flags needed — this example is guard-clean without `--auto-guard`.
