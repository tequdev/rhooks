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
| [`emit-txn`](emit-txn) | `etxn_reserve` + a `txn_template!`-declared Payment/`emit`, with a `cbak` |

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
- A `rollback!`/`accept!` exit that carries a meaningful (non-zero, non-`-1`
  placeholder) code defines its codes with `hooks_lib::hook_errors!` rather
  than bare integer literals — see `firewall`, `state-counter`, and
  `emit-txn` for worked examples, and each crate's own README for its error
  code table.
- Loops carry `guard!`/`guard_m!` when the bound is known at the source
  level. Some loops in the compiled output are *not* written in the
  source at all — see "On `--auto-guard`" below.
- Runtime arithmetic (`+`, `-`, `*`, ...) on non-constant values is
  avoided; `clippy::arithmetic_side_effects` is `warn` in `[lints]`, but
  the workspace's `-D warnings` clippy invocation promotes it to a hard
  error (a specific lint's explicit level wins over the command-line
  `warnings` group). Use `.wrapping_add()`/`.checked_add()`/etc. instead
  of bare operators wherever a runtime value is involved.

## Statics for templates and large buffers

Constant byte templates and large output buffers should be `static`s, not
stack locals (see `emit-txn` for the worked example):

- a stack-local array literal is materialized at runtime by a chain of
  store instructions (code bytes + worst-case instruction count), while a
  `static` template becomes a wasm **data segment** costing exactly its
  own bytes;
- a stack `[0u8; N]` for large `N` compiles to a `compiler_builtins`
  memset loop (an unguarded loop you never wrote), while a zero-initialized
  `static` lands in linear-memory **BSS** — zero bytes of data segment,
  zero code, because wasm memory is zero-initialized by definition.

Use `hooks_lib::static_cell::HookStatic` (in the prelude) rather than a
raw `static mut`: `HookStatic::new(...)` is `const` (so the data placement
above still applies), and `take()` hands out the buffer's one exclusive
`&'static mut` safely — the second `take()` returns `None`, so aliasing is
structurally impossible and no `unsafe` appears in hook code. Exclusivity
is sound because hooks execute single-threaded and every invocation runs
in a freshly instantiated wasm instance.

Converting `emit-txn` to this idiom removed its only compiler-generated
loops entirely (no `--auto-guard` needed) and cut its worst-case
instruction count by an order of magnitude (6798 → ~350 as of the current
toolchain; exact numbers drift a little between compiler versions — the
`hooks-build build` output prints the authoritative figures). The
take-once flag costs a few dozen bytes over a raw `static mut` — the
price of keeping hook code free of `unsafe`.

## On `--auto-guard`

`hooks-build build` defaults to treating an unguarded `loop` as a hard
error (see `docs/DESIGN.md` §6.3 and §10.1) — missing a `guard!` in your
own code is a bug, not something to paper over. In practice, though, one
of these four examples fails that check even though **no loop appears in
its Rust source**: `opt-level = "z"` on `wasm32v1-none` (which has no
bulk-memory instructions) causes LLVM to lower some operations to calls
into `compiler_builtins` functions that contain real, unguarded loops:

- `firewall`'s `sender == blocked` (`[u8; 20]` equality) compiles to a
  byte-by-byte compare loop, so it needs `--auto-guard`.

Critically, the `--auto-guard` default of `--default-maxiter 16` is **not
safe** for it: the account-compare loop can run up to 20 iterations (one
per `AccountId` byte). An auto-inserted guard whose `maxiter` is smaller
than the loop's true worst case builds fine (`hooks-build` only checks
module *shape*, not runtime behavior) but risks a real `GUARD_VIOLATION`
on a live node. The `build-examples` task therefore passes an explicit,
reasoned `--default-maxiter 24` instead of the default — see the task in
the root `mise.toml` and `firewall`'s README.

`accept-all`, `state-counter`, and `emit-txn` build clean with no extra
flags: the first two have no compiler-generated loops (no buffer
copy/compare in them is large enough, at this optimization level, for
LLVM to prefer an out-of-line loop over inline stores), and `emit-txn`
avoids them via the static-buffer idiom above.
