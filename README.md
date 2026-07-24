# rhooks

A Rust monorepo for developing [Xahau](https://xahau.network/) Hooks
(WebAssembly smart contracts) end to end — from raw Hook API bindings to a
SetHook-valid `.wasm` binary.

See [`docs/DESIGN.md`](docs/DESIGN.md) for the full design.

## Crates

| crate | description |
|---|---|
| `hooks-core` | `no_std`, zero-logic FFI layer: raw Hook API declarations and every constant from the xahaud `hook/` headers, translated 1:1 into Rust. |
| `hooks-lib` | `no_std`, ergonomic wrapper over `hooks-core` (`Result`-based APIs, typed buffers, XFL type, guard/trace macros, panic handler). |
| `hooks-build` | CLI that turns a Rust crate into a SetHook-valid WASM binary (cargo build + hook-cleaner + guard-checker, natively in Rust). |

`examples/` (a separate workspace) holds runnable Hooks built with
`hooks-lib`.

## Building

```sh
mise run build-wasm   # builds the no_std crates for wasm32v1-none
mise run lint         # cargo clippy --workspace --all-targets -- -D warnings
mise run fmt          # cargo fmt --all
mise run test         # cargo test --workspace
```

## Examples

| example | demonstrates |
|---|---|
| [`accept-all`](examples/accept-all) | minimal hook: `accept` everything (starter template) |
| [`firewall`](examples/firewall) | read `otxn_field(sfAccount)` + a hook parameter blacklist → `rollback` |
| [`state-counter`](examples/state-counter) | `state`/`state_set` round-trip, counter in hook state |
| [`emit-txn`](examples/emit-txn) | `etxn_reserve` + a user-declared `txn_template!` Payment, with a `cbak` |

```sh
mise run build-examples   # builds all four through hooks-build and checks the output
```

See [`examples/README.md`](examples/README.md) for details, including why
one of the four (`firewall`) needs `--auto-guard`.

## E2E tests

`e2e/` deploys the four examples' `hooks-build` output to a real,
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
