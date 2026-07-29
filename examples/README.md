# examples/

Runnable Xahau Hooks written with `hooks-lib`, buildable with `hooks-build`.
This is its own Cargo workspace (see `Cargo.toml`), separate from the root
workspace, because these crates are `no_std` `cdylib`s with a Hook-specific
release profile that must not leak into `hooks-core`/`hooks-lib`/
`hooks-build`, and they don't build for host targets.

Directories are numbered in **suggested reading order** — start at
`01_accept-all` and work down; each one builds on ideas from the examples
before it (state, then params, then errors, then a real filter, then
guards, then XFL, then slots, then foreign state, then emission). The
`example` column below is each crate's actual package name (unchanged by
the numbering — Cargo package names can't start with a digit, so only the
directory is prefixed) and matches what its own README, `Cargo.toml`, and
`use` statements call it.

| # | example | demonstrates |
|---|---|---|
| 01 | [`accept-all`](01_accept-all) | minimal hook: `accept` everything (starter template) |
| 02 | [`state-counter`](02_state-counter) | `state`/`state_set` round-trip, counter in hook state |
| 03 | [`hook-params`](03_hook-params) | `hook_param`-configurable threshold, with a compiled-in default |
| 04 | [`errors`](04_errors) | a meaningful `hook_errors!`-based rollback error-code system, matched to `HookReturnCode` |
| 05 | [`firewall`](05_firewall) | read `otxn_field(sfAccount)` + a hook parameter blacklist → `rollback` |
| 06 | [`guard-patterns`](06_guard-patterns) | `guard!`/`guard_m!` correctness, choosing `maxiter`, and the array-`==` memcmp-loop pitfall |
| 07 | [`xfl-math`](07_xfl-math) | reading `Amount` as XFL (`slot_float`/`sto_set`), `mulratio`, checked `Add`/`Sub`/`Mul`/`Div`/`Neg` operators, `.compare()`-family methods, and `XFLUnchecked`'s hot-path chain |
| 08 | [`slot-ledger`](08_slot-ledger) | `otxn_slot`/`slot_subfield`/`slot`/`slot_size`: transaction field access via slots |
| 09 | [`state-foreign`](09_state-foreign) | `state_foreign`: reading another (hook-parameter-configured) account's hook state |
| 10 | [`emit-txn`](10_emit-txn) | `etxn_reserve` + a `txn_template!`-declared Payment/`emit`, with a `cbak` |
| 12 | [`typed-data`](12_typed-data) | `#[derive(HookData)]`: composite (multi-field) state keys/values and `otxn_param`/`hook_param` structs, in place of hand-packed byte buffers |
| 14 | [`account-id-macro`](14_account-id-macro) | `hooks_lib::account_id!`: compile-time r-address -> `AccountId` decode, cross-checked against `hook_account`/`util_accid`/`util_raddr` |

(11 and 13 are reserved for other in-flight work — see git history/other
branches — hence the jump from `12` to `14`.)

## 80+: Production hooks in Rust

Unlike `01`-`10` (one concept each, in suggested reading order), the
`80`+ series are behavior-equivalent Rust ports of real, deployed xahaud
C hooks — read them after `01`-`10`, not instead of them. Each has its
own README with a full behavior-equivalence table against its C source,
a differences table for any intentional deviation, and a "Toolchain
limitation" section documenting a real Guard-type nesting-depth/
floating-point constraint discovered while porting them (see
`80_reward`'s README for the first, fullest writeup; `81_govern`'s adds
three more findings).

| # | example | ports |
|---|---|---|
| 80 | [`reward`](80_reward) | [`hook/genesis/reward.c`](https://raw.githubusercontent.com/Xahau/xahaud/dev/hook/genesis/reward.c) — the `RewardHook`: computes and emits a `GenesisMint` crediting `ClaimReward` claimants and active-validator L1 seats |
| 81 | [`govern`](81_govern) | [`hook/genesis/govern.c`](https://raw.githubusercontent.com/Xahau/xahaud/dev/hook/genesis/govern.c) — the `GovernanceHook`: the 20-seat L1/L2 round-table governance state machine |

## Entry points: `#[hook]` / `#[cbak]`

Every example declares its entry point as a plain, safe function annotated
with `hooks_lib::hook` (or `hooks_lib::cbak` for the optional settlement
callback), not a hand-written `extern "C"` export:

```rust
use hooks_lib::hook;

#[hook]
fn my_hook() -> i64 {
    // ...
}
```

`#[hook]` expands to the wasm export shape the Hook host requires
(`#[unsafe(no_mangle)] pub extern "C" fn hook(_reserved: u32) -> i64`,
calling the annotated function) — see `hooks-macros`'s crate doc comment
for the exact signature it enforces (no arguments, `-> i64`, no
`async`/`unsafe`/`const`/`extern`/generics). The annotated function's own
name is arbitrary (`my_hook` here is just a convention); what matters is
the export it produces.

## Building

```sh
mise run build-examples   # builds every example through hooks-build and checks the output
```

This is also the toolchain's end-to-end test: each example is built via
`cargo run -p hooks-build -- build ...` from the root workspace, and the
resulting `out/<name>.wasm` is re-validated with `hooks-build check`.

Each example can also be built individually, e.g.:

```sh
cargo run -p hooks-build -- build --manifest-path examples/02_state-counter/Cargo.toml
```

See each example's own README for its exact command — none currently need
`--auto-guard` (see below for why, and when it would still be needed).

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
instruction count by an order of magnitude (6798 → 331 as of the current
toolchain and this workspace's `opt-level = 3` default, see
`docs/DESIGN.md` §2 C6; exact numbers drift a little between compiler
versions and profile settings — the `hooks-build build` output prints the
authoritative figures). The take-once flag costs a few dozen bytes over a
raw `static mut` — the
price of keeping hook code free of `unsafe`.

## On `--auto-guard`

`hooks-build build` defaults to treating an unguarded `loop` as a hard
error (see `docs/DESIGN.md` §6.3 and §10.1) — missing a `guard!` in your
own code is a bug, not something to paper over. The trap is that
`opt-level = "z"` on `wasm32v1-none` (which has no bulk-memory
instructions) can cause LLVM to lower some operations to calls into
`compiler_builtins` functions that contain real, unguarded loops **even
though no loop appears in the Rust source at all** — array/slice equality
(`[u8; N] == [u8; N]`) lowers to a `bcmp`-style byte-compare loop, and large
buffer zero-inits/copies lower to `memset`/`memcpy`-style loops.

`--auto-guard` (with a carefully sized `--default-maxiter`) is one way to
handle this, but it is a footgun: the CLI only validates guard *shape*, not
that `maxiter` covers the loop's true runtime bound, so an under-sized
`maxiter` builds clean and then fails with `GUARD_VIOLATION` on a live
node. Two source-level idioms avoid the compiler-generated loop (and the
`--auto-guard` footgun) entirely, and are preferred wherever they apply:

- **Fixed-size buffer equality**: use `hooks_lib::buf_eq_8`/`_20`/`_32`/
  `_33`/`_34`/`_40`/`_48`/`_64` (see `crates/hooks-lib/src/buf_eq.rs`) instead
  of `==`. Each function compares its buffer as a fixed sequence of
  word-sized (`u64`, with a narrower tail word where the size isn't a
  multiple of 8) chunks built from source-level literal byte indices, so the
  comparison is genuinely straight-line code — there is nothing for LLVM to
  lower into a loop. `firewall` used to need
  `--auto-guard --default-maxiter 24` for exactly this reason (its
  `sender == blocked` account comparison); switching to `buf_eq_20` removed
  the loop (and the flag) entirely, and the word-at-a-time comparison
  further dropped `firewall`'s worst-case instruction count from 419 to 122.
- **Statics for templates and large buffers** (below): removes
  compiler-generated `memset`/`memcpy` loops the same way, for the
  initialization/copy case `buf_eq` doesn't cover.

None of these examples need `--auto-guard`: `accept-all` and
`state-counter` never had a compiler-generated loop to begin with (no
buffer copy/compare in them is large enough, at this optimization level,
for LLVM to prefer an out-of-line loop over inline stores); `emit-txn`
avoids one via the static-buffer idiom below; `firewall` avoids one via
`buf_eq_20` above; `hook-params`, `errors`, `xfl-math`, and `state-foreign`
have no buffer copy/compare large enough either; `slot-ledger` avoids one
by checking `slot_size` before sizing its read buffer instead of always
allocating room for the larger of the two `Amount` encodings; `guard-patterns`'
only loops are hand-written and already guarded in the source (see its own
README for why, including an empirical check of what `guard_m!`'s `$n`
does and doesn't protect against); and `account-id-macro`'s buffers (a
20-byte `AccountId`, a 34-byte r-address) are compared with `buf_eq_20`/
`buf_eq_34` and are far too small for LLVM to prefer an out-of-line loop
regardless. `--auto-guard` remains
available in `hooks-build` for cases none of these idioms cover — size
`--default-maxiter` from the loop's true worst-case iteration count (found
via disassembly), never trust the default.
