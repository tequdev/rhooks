# Your First Hook

This chapter walks through `accept-all`, the minimal starter Hook: it
traces a short message, then unconditionally accepts the transaction that
triggered it. No loops, no state, no emitted transactions — a good template
to copy for a new Hook and a good way to see every required piece in one
small file.

## The source

Create `src/lib.rs` in the crate you set up in [Installation](installation.md):

```rust
#![no_std]

use rshooks::*;

metadata! {
    name: "accept-all",
    description: "Accepts every transaction selected by HookOn.",
    HookOn: [Invoke],
    HookName: "accept",
}

#[hook]
fn my_hook() -> i64 {
    trace!(b"accept-all: accepting transaction");
    accept!()
}
```

To see the `trace!` line actually run, enable the `trace` feature in
`Cargo.toml` alongside `rshooks`:

```toml
[dependencies]
rshooks = { version = "0.1", features = ["trace", "host-panic-handler"] }
```

### `#![no_std]`

Every Hook crate is `#![no_std]`: there's no allocator and no `std` on the
Hook host, and `rshooks` itself is `no_std` so it can be linked into one.

### `use rshooks::*;`

This glob import brings in everything declared at `rshooks`'s crate root:
the `#[hook]`/`#[cbak]` attribute macros, the `metadata!`/`XFL!`/
`account_id!` macros, the `accept!`/`rollback!`/`trace!`/`guard!` macro
family, and every top-level module (`api`, `types`, `xfl`, ...) by name.
It does **not** bring the functions inside those modules into scope — a
Hook that calls typed API functions like `otxn_field` or `state` also
needs `use rshooks::prelude::*;`, which this minimal example doesn't, since
it never reads the transaction or touches state. Later chapters that do
add that import.

### `metadata!`

The `metadata!` block declares build-only information about the Hook: a
display name, an optional description, which transaction types trigger it
(`HookOn`), and (optionally) its on-ledger `HookName`. It's not required —
a Hook without it still builds and runs — but `rshooks` uses it to
generate a JSON sidecar describing the binary. See [Hook
Metadata](../build/metadata.md) for the full grammar.

### `#[hook]`

`#[hook]` turns a plain, argument-less `fn my_hook() -> i64` into the wasm
export the Hook host requires. It expands to:

```rust
#[unsafe(no_mangle)]
pub extern "C" fn hook(_reserved: u32) -> i64 {
    my_hook()
}
```

The annotated function must take no arguments and return `i64`, with no
`async`/`unsafe`/`const`/`extern` modifiers and no generics — `#[hook]`
rejects anything else at compile time with a pointed error rather than
producing a malformed export. The function's own name (`my_hook` here) is
just a convention; what matters is the `hook` export it produces. Use
`#[cbak]` the same way to export the optional settlement callback,
`cbak`, invoked when a transaction this Hook previously emitted settles.

### `accept!()`

`accept!()` calls the host's `accept` function and never returns — its
return type is `!`. `accept!(msg, code)` additionally carries a trace
message and a caller-chosen result code; the bare form used here accepts
with no message and code `0`. Its counterpart, `rollback!(msg, code)`,
rejects the transaction instead. Both are covered in more depth in [Accept,
Rollback, and Errors](../concepts/errors.md).

## Building it

From the crate's own directory:

```sh
rshooks build
```

or from elsewhere, pointing at its manifest:

```sh
rshooks build --manifest-path my-hook/Cargo.toml
```

This runs `cargo build --release --target wasm32v1-none`, then
post-processes the resulting `.wasm` — see [Building a
Hook](building.md) for exactly what that post-processing does. A
successful build prints something like:

```text
worst-case instructions: hook=15 cbak=0
max nesting depth: 0
wrote out/my_hook.wasm
size: 174 bytes
estimated SetHook fee: 870000 drops (0.870000 XAH)
wrote out/my_hook.json
```

## What lands in `out/`

Two files appear next to your crate's `Cargo.toml`:

- **`out/my_hook.wasm`** — the cleaned, SetHook-valid binary: cargo's raw
  `cdylib` output with the `memory` export stripped and every Hook API
  rule (§ single `hook`/`cbak` export, guarded loops, MVP-only
  instructions) validated.
- **`out/my_hook.json`** — the metadata sidecar, generated because this
  crate declared `metadata!`. For `accept-all` specifically, it looks like:

```json
{
  "name": "accept-all",
  "description": "Accepts every transaction selected by HookOn.",
  "HookOn": "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF7FFFFFFFFFFFFFFFFFFBFFFFF",
  "HookCanEmit": null,
  "HookName": "616363657074",
  "HookHash": "DCE6A3F81224AE89C557F04D73420D808D9009BCF1CFC1474396CD2DA2D4DF16",
  "WCE": {
    "hook": 15,
    "cbak": 0
  },
  "human": {
    "HookOn": [
      "Invoke"
    ],
    "HookCanEmit": null,
    "HookName": "accept"
  }
}
```

The **`WCE`** (worst-case execution) numbers are the static, guard-derived
upper bound on instructions the host will ever execute for `hook`/`cbak` —
the same figures the pipeline printed to the terminal. The **`HookHash`**
is Xahau's hash of the deployed binary: the uppercase hex of the first 32
bytes of the wasm's SHA-512 digest — this is what identifies the exact
Hook code on-ledger, independent of which account installed it. Both are
computed from the final, cleaned `.wasm` bytes, so they only exist once a
build has run.

From here, [Building a Hook](building.md) explains what each pipeline
stage actually does, and [The `rshooks` CLI](../build/cli.md) is the
complete flag reference for every subcommand.
