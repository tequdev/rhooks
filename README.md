# rshooks

A Rust monorepo for developing [Xahau](https://xahau.network/) Hooks
(WebAssembly smart contracts) end to end — from raw Hook API bindings to a
SetHook-valid `.wasm` binary.

See [`docs/DESIGN.md`](docs/DESIGN.md) for the full design.

## Crates

| crate | description |
|---|---|
| `rshooks-core` | `no_std`, zero-logic FFI layer: raw Hook API declarations and every constant from the xahaud `hook/` headers, translated 1:1 into Rust. |
| `rshooks-macros` | Procedural macros for `rshooks` (declarations, metadata, XFL literals). |
| `rshooks` | `no_std`, ergonomic wrapper over `rshooks-core` (`Result`-based APIs, typed buffers, XFL type, guard/trace macros, panic handler). |
| `rshooks-build` | CLI that turns a Rust crate into a SetHook-valid WASM binary (cargo build + hook-cleaner + guard-checker, natively in Rust). |

`examples/` (a separate workspace) holds runnable Hooks built with
`rshooks`.

## Installation

Hook crates depend on [`rshooks`](https://crates.io/crates/rshooks); the
build CLI installs with `cargo install rshooks-build`, which installs a
binary named `rshooks` (run as `rshooks build`, `rshooks check`, `rshooks
clean`).

## Building

```sh
mise run build-wasm   # builds the no_std crates for wasm32v1-none
mise run lint         # cargo clippy --workspace --all-targets -- -D warnings
mise run fmt          # cargo fmt --all
mise run test         # cargo test --workspace
```

## Examples

Numbered in suggested reading order — see
[`examples/README.md`](examples/README.md) for the full walkthrough of why.

| # | example | demonstrates |
|---|---|---|
| 01 | [`accept-all`](examples/01_accept-all) | minimal hook: `accept` everything (starter template) |
| 02 | [`state-counter`](examples/02_state-counter) | `state`/`state_set` round-trip, counter in hook state |
| 03 | [`hook-params`](examples/03_hook-params) | `hook_param`-configurable threshold, with a compiled-in default |
| 04 | [`errors`](examples/04_errors) | a meaningful `hook_errors!`-based rollback error-code system, matched to `HookReturnCode` |
| 05 | [`firewall`](examples/05_firewall) | read `otxn_field(sfAccount)` + a hook parameter blacklist → `rollback` |
| 06 | [`guard-patterns`](examples/06_guard-patterns) | `guard!`/`guard_m!` correctness, choosing `maxiter`, and the array-`==` memcmp-loop pitfall |
| 07 | [`xfl-math`](examples/07_xfl-math) | reading `Amount` as XFL, `mulratio`, checked XFL operators, and `XFLUnchecked`'s hot-path chain |
| 08 | [`slot-ledger`](examples/08_slot-ledger) | `otxn_slot`/`slot_subfield`/`slot`/`slot_size`: transaction field access via slots |
| 09 | [`state-foreign`](examples/09_state-foreign) | `state_foreign`: reading another account's hook state |
| 10 | [`emit-txn`](examples/10_emit-txn) | `etxn_reserve` + a user-declared `txn_template!` Payment, with a `cbak` |

```sh
mise run build-examples   # builds all ten through rshooks-build and checks the output
```

Each Hook can declare build-only metadata next to its entry point:

```rust
metadata! {
    name: "emit-txn",
    description: "Emits a Payment and handles its callback.",
    HookOn: [Invoke],
    HookCanEmit: [Payment],
    HookName: "emit-tx",
}
```

`rshooks build` writes a matching `.json` sidecar beside the cleaned
`.wasm`. Its top-level SetHook fields use deployable raw values (transaction
masks and hex `HookName`); the readable declarations are under `human`. The
sidecar also includes the final binary's `HookHash` and static `WCE`
(`hook`/`cbak`) values. Metadata is carried only through an unreachable
raw-WASM export that the cleaner removes, so it does not change the final
WASM bytes, hash, or instruction count.

See [`examples/README.md`](examples/README.md) for details, including the
compiler-generated-loop pitfall that used to require `--auto-guard` (none
of the ten examples need it any more).

## E2E tests

`e2e/` deploys the examples' `rshooks-build` output to a real,
standalone `xahaud` (via `SetHook`) and asserts on the resulting
transaction metadata and ledger state — proof of runtime behavior, not
just that the binaries are SetHook-valid. See
[`docs/E2E-TESTING.md`](docs/E2E-TESTING.md) for the design.

```sh
mise run e2e:node-up     # starts a standalone Xahau node (xrpld-netgen; needs Docker)
mise run e2e              # builds the examples, then runs the e2e suite against it
mise run e2e:node-down   # stops it
```

`e2e/` is an isolated pnpm package (not part of any Cargo or pnpm
workspace) using the same stack as this machine's other hook repos:
vitest + `@transia/hooks-toolkit` + `xahau`.
