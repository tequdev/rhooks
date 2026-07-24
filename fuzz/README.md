# `hooks-build-fuzz` — cargo-fuzz targets

`cargo fuzz` (libFuzzer) property/fuzz targets for the host-independent pure
logic in this workspace (`docs/DESIGN.md` §8, "PLAN L3"). This is a separate
Cargo workspace (`[workspace] members = ["."]` in `Cargo.toml`), deliberately
**not** a member of the root workspace (`members = ["crates/*"]`) — cargo-fuzz
targets always live in their own workspace so their special build flags
(ASan, sanitizer coverage instrumentation) never leak into normal builds.

## Prerequisites

- `cargo install cargo-fuzz` (not part of the pinned toolchain's default
  component set — install once per machine).
- **Nightly is required**, unlike the rest of this workspace: `cargo fuzz`
  itself needs nightly for `-Z sanitizer=address`, independent of anything
  this repo does. The repository's pinned toolchain
  (`rust-toolchain.toml`, currently `nightly-2026-07-20`) already satisfies
  this — no separate nightly install is needed, and no other lane in this
  workspace needs to move off stable for fuzzing to keep working. If a
  future change stabilizes the rest of the workspace onto a stable
  toolchain, this `fuzz/` directory can keep pinning/using nightly
  independently (e.g. via `cargo +nightly fuzz ...`) without affecting
  anything else.

## Targets

- **`xfl_decode`** — `hooks_lib::xfl::XFL`'s non-host-dependent decode path
  (`from_raw_bits` -> `exponent()`, plus panic-freedom of the host-call-
  backed accessors on this non-`wasm32` host). Property: never panics, and
  `exponent()`'s bias-97 field decode always matches an independent
  recomputation, for every possible `i64`.
- **`txn_codec`** — `hooks_lib::txn::codec::field_header` and the field-size
  helpers built on it, for arbitrary `u32` `sfcode` input. Property: never
  panics, output length matches the documented 1/2/3-byte encoding table,
  and the size helpers stay consistent with `field_header`'s own length.
- **`build_pipeline`** — `wasm-smith`-generated modules (under a `Config`
  approximating a SetHook module's constraints: MVP-only instructions/types,
  single memory, no imports) run through `hooks_build::clean` ->
  `flatten` -> `unnest`. Properties: no panics, `wasmparser`-valid output at
  every stage, and (when the pre-pipeline module doesn't itself trap) the
  `hook` export's return value is unchanged by the pipeline — a randomized
  counterpart to `crates/hooks-build/tests/{flatten,unnest}_differential.rs`.

## Running

```sh
cargo fuzz run --fuzz-dir fuzz xfl_decode
cargo fuzz run --fuzz-dir fuzz txn_codec
cargo fuzz run --fuzz-dir fuzz build_pipeline
```

(or `cd fuzz && cargo fuzz run <target>` — `--fuzz-dir` just lets it run from
the repo root, since the root has no top-level `[package]` for cargo-fuzz to
anchor off of).

`mise run fuzz-smoke` runs all three for a short, bounded time (CI-style
smoke check that the harnesses still build and execute — not a substitute
for a real fuzzing campaign). It is intentionally **not** part of
`mise run test`.

## Known finding: `txn_codec` (not fixed here)

`txn_codec` reliably finds a pre-existing panic in
`hooks_lib::txn::codec::field_header`
(`crates/hooks-lib/src/txn.rs`) within the first few runs: for an `sfcode`
whose `type` or `field` component (`sfcode >> 16` / `sfcode & 0xFFFF`) is
`>= 256`, the function hits one of its `assert!`s (e.g. `sfcode =
0x4832_0A8A`, type `18482`, field `2698`) and panics, even though
`field_header` is documented as a "generic, panic-free" primitive with no
`# Panics` section of its own (unlike its const-context-only-panic siblings
`write_field_header`/`write_const_bytes`). Every real `sfcode`
(`hooks_core::sfcodes`) keeps both components under `256`, so this never
triggers on real transaction fields — but the function's own public,
non-`const` signature accepts any `u32`.

This is **not fixed in this change**: `crates/hooks-lib/src/txn.rs` is owned
by a different work lane (`impl-txn-macro`/`impl-txn-template`). See the PR
description for the full repro and a suggested fix shape (return `Option`/
`Result` instead of asserting, or explicitly document+`debug_assert!` the
precondition if only `const`-context callers are meant to hit it).

## Known finding: `build_pipeline` (not fixed here)

`build_pipeline` finds a real `hooks_build::flatten` bug within roughly
100-150k executions (a handful of seconds): `flatten` rebuilds a module's
type section down to exactly `{import types} ∪ {entry type}`
(`docs/DESIGN.md` §6.2b), but does not rewrite (or otherwise account for)
type indices used as an explicit, non-empty `blocktype` on a `block`/
`loop`/`if` instruction inside a function body — only call-target function
types are considered. A minimal repro: a `hook` body containing

```wat
(block (type $t) ... end)  ;; $t : (i32, i32) -> i32, distinct from hook's own type
```

survives `clean` (which doesn't touch the type section's contents this way)
but after `flatten` rebuilds the type section to just the entry type
`(i32) -> i64` plus `_g`'s `(i32, i32) -> i32`, the block's *type index* is
left unchanged — so it now silently refers to whatever type landed at that
new index (typically the just-inserted `_g` import type), producing a
`block` with the wrong param/result arity for its (empty) contents. The
resulting module fails `wasmparser::validate` (e.g. "unknown type: type
index out of bounds", or a stack-mismatch error, depending on which index
collision the corpus happens to hit).

This is **not fixed in this change**: `crates/hooks-build/src/flatten.rs` is
owned by a different work lane (`impl-flatten`). See the PR description for
a byte-exact repro (`cargo fuzz run` input) and the `wat` before/after
dump.
