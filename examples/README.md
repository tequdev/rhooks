# examples/

Runnable Xahau Hooks written with `hooks-lib`, buildable with `hooks-build`.
This is its own Cargo workspace (see `Cargo.toml`), separate from the root
workspace, because these crates are `no_std` `cdylib`s with a Hook-specific
release profile that must not leak into `hooks-core`/`hooks-lib`/
`hooks-build`, and they don't build for host targets.

| example | demonstrates |
|---|---|
| [`accept-all`](accept-all) | minimal hook: `accept` everything (starter template) |
| [`firewall`](firewall) | read `otxn_field(sfAccount)` + a hook parameter blacklist → `rollback` |
| [`state-counter`](state-counter) | `state`/`state_set` round-trip, counter in hook state |
| [`emit-txn`](emit-txn) | `etxn_reserve` + `prepare`/`emit` a Payment, with a `cbak` |

## Building

```sh
mise run build-examples   # builds all four through hooks-build and checks the output
```

This is also the toolchain's end-to-end test: each example is built via
`cargo run -p hooks-build -- build ...` from the root workspace, and the
resulting `out/<name>.wasm` is re-validated with `hooks-build check`.

Each example can also be built individually, e.g.:

```sh
cargo run -p hooks-build -- build --manifest-path examples/state-counter/Cargo.toml
```

See each example's own README for its exact command (some need
`--auto-guard`; see below).

## Source style rules

These are enforced by the examples workspace's `[lints]` (mirroring the
root workspace's panic-free set) and by review:

- No slice indexing or range-slicing with a **non-literal** index — it can
  panic. Use `.get()`/`.get_mut()` (returns `Option`) instead. Indexing or
  range-slicing a fixed-size array with a **literal, provably-in-bounds**
  index is fine and is used freely in these examples (`clippy::
  indexing_slicing` only rejects indexing/slicing it cannot prove safe).
- No `format!`/`core::fmt` — `trace!`/`accept!`/`rollback!` take raw byte
  slices, not formatted strings.
- No `unwrap`/`expect`/`panic!` (all denied by `[lints]`); handle every
  `Result` explicitly, typically by rolling back on `Err`.
- Loops carry `guard!`/`guard_m!` when the bound is known at the source
  level. Some loops in the compiled output are *not* written in the
  source at all — see "On `--auto-guard`" below.
- Runtime arithmetic (`+`, `-`, `*`, ...) on non-constant values is
  avoided; `clippy::arithmetic_side_effects` is `warn` in `[lints]`, but
  the workspace's `-D warnings` clippy invocation promotes it to a hard
  error (a specific lint's explicit level wins over the command-line
  `warnings` group). Use `.wrapping_add()`/`.checked_add()`/etc. instead
  of bare operators wherever a runtime value is involved.

## On `--auto-guard`

`hooks-build build` defaults to treating an unguarded `loop` as a hard
error (see `docs/DESIGN.md` §6.3 and §10.1) — missing a `guard!` in your
own code is a bug, not something to paper over. In practice, though, two
of these four examples fail that check even though **no loop appears in
their Rust source**: `opt-level = "z"` on `wasm32v1-none` (which has no
bulk-memory instructions) causes LLVM to lower some operations to calls
into `compiler_builtins` functions that contain real, unguarded loops:

- `firewall`'s `sender == blocked` (`[u8; 20]` equality) compiles to a
  byte-by-byte compare loop.
- `emit-txn`'s zero-initialization of its 320-byte `prepared` buffer
  compiles to an 8-bytes-per-iteration `memset`-style loop.

Both need `--auto-guard`. Critically, the `--auto-guard` default of
`--default-maxiter 16` is **not safe** for either: the account-compare
loop can run up to 20 iterations (one per `AccountId` byte) and the
`memset` loop can run up to ~40 (`320 / 8`, plus small head/tail loops).
An auto-inserted guard whose `maxiter` is smaller than the loop's true
worst case builds fine (`hooks-build` only checks module *shape*, not
runtime behavior) but risks a real `GUARD_VIOLATION` on a live node. The
`build-examples` task therefore passes an explicit, reasoned
`--default-maxiter` for each (24 and 48 respectively) instead of the
default — see the task in the root `mise.toml` for the exact commands and
reasoning, and each example's own README.

`accept-all` and `state-counter` have no such compiler-generated loops (no
buffer copy/compare in them is large enough, at this optimization level,
for LLVM to prefer an out-of-line loop over inline stores) and build clean
with no extra flags.
