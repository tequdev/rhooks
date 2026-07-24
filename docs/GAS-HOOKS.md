# Gas-type hook development (HookApiVersion 1)

Status: FIRST PASS (PLAN L6, 2026-07-24) — `hooks-build`'s v1 validator
rules brought up to parity with what `GasValidator.cpp` actually enforces,
plus `examples/gas-counter` as the first Gas-type example. hooks-lib itself
gained no v1-specific API in this pass (see "What did *not* change" below).

This document is the developer-facing counterpart to `docs/DESIGN.md`
§6.2–6.4 (which describe the pipeline implementation); read it if you are
writing a Gas-type hook, not extending `hooks-build` itself.

## What a Gas-type hook is

HookApiVersion 0 ("Guard-type") hooks are the only kind this toolchain
targeted until now: every `loop` in the compiled wasm must carry a static
`_g` guard call (`docs/DESIGN.md` §6.3), because xahaud's SetHook validation
for v0 runs a *static* worst-case instruction-count analysis
(`validateGuards`/`check_guard`, `Guard.h`) before the hook is ever allowed
on-ledger.

HookApiVersion 1 ("Gas-type") hooks replace that static analysis with
runtime metering: the hook's wasm is executed under a WasmEdge instruction
cost meter, drawing down a gas pool the *transaction* funds
(`sfHookGas`), not the hook definition. There is no static bound to prove at
SetHook time, so:

- Ordinary Rust loops and array comparisons need no `guard!`/`guard_m!` and
  no `--auto-guard` build step — see `examples/gas-counter`.
- The wasm must **not** import `_g` at all — it would be meaningless (there
  is nothing to guard against) and xahaud's Gas-type validator
  (`GasValidator.cpp`) hard-rejects it outright.
- `hooks-build`'s `flatten`/`unnest` passes (§6.2b/§6.2c) do not run for
  `--api-version 1` — those exist solely to satisfy the v0 guard checker's
  structural requirements (single entry-point function, bounded nesting
  depth), which do not apply here. Compiled functions can stay as ordinary,
  non-inlined calls.

## Building a Gas-type hook

Every `hooks-build` subcommand that touches validation takes
`--api-version 1`:

```sh
cargo run -p hooks-build -- build --manifest-path examples/gas-counter/Cargo.toml --api-version 1
cargo run -p hooks-build -- check examples/gas-counter/out/gas_counter.wasm --api-version 1
```

Omitting `--api-version 1` validates against the v0 (Guard-type) rule set
instead, which a Gas-type hook will generally fail (missing `_g` import,
unguarded loops) — the flag is not optional, and there is no
auto-detection.

On the Rust side, nothing about writing the hook itself changes: the same
`hooks-lib` (`state`/`state_set`, `accept!`/`rollback!`, `pad!`, XFL, ...)
works unmodified for a v1 hook, simply because none of its own code paths
call `guard!`/`guard_m!` (see `examples/gas-counter/src/lib.rs`'s doc
comment for the one thing that *is* different: your own loops need no
annotation).

## SetHook fields for Gas-type hooks

| Field | Where | Required | Notes |
|---|---|---|---|
| `sfHookApiVersion` | `Hook` object (CREATE) | Yes, `= 1` | Immutable once set; UPDATE cannot change it. |
| `sfHookGas` | Transaction (any tx that triggers the hook chain) | Yes | The strong-execution gas pool for the *whole hook chain*, not per-hook: each hook in the chain receives the entire remaining pool and its cost is subtracted before the next hook runs. |
| `sfHookCallbackGas` | `Hook` object / HookDefinition | Only if the wasm exports `cbak()` | Gas budget for callback execution; drawn from a separate pool, not the strong pool. |
| `sfHookWeakGas` | `Hook` object / HookDefinition | Only if the `hsfCOLLECT` flag is set | Gas budget for weak (post-apply) execution; likewise not drawn from the strong pool. |

Fee impact: 1 unit of any of the three gas fields above costs 1 drop, in
addition to the base fee — a large `sfHookGas` on every triggering
transaction is a real, ongoing cost, unlike v0's guard-based model where the
worst-case instruction count only affects the *hook's own* fee estimate at
SetHook time.

`examples/gas-counter`'s `hooks-build build` output prints its size and
estimated *SetHook* fee (from `hooks_build::estimate_fee`, unchanged for
v1); it does not — and cannot — estimate `sfHookGas`, since that number
depends on runtime gas cost, not static analysis. Size it empirically
against a live node.

## Failure modes specific to Gas-type

- **`tecHOOK_INSUFFICIENT_GAS`**: the strong-execution gas pool
  (`sfHookGas`) was exhausted mid-chain. Raise `sfHookGas` on the triggering
  transaction.
- **`temMALFORMED` at SetHook time**: `sfHookApiVersion` missing on CREATE;
  `sfHookCallbackGas` present without a `cbak()` export (or vice versa);
  `sfHookWeakGas` present without `hsfCOLLECT`; or the wasm itself fails
  `GasValidator.cpp`'s checks (see below) — none of these are things
  `hooks-build check --api-version 1` can fully replicate (see "Validation
  coverage and its limits").

## Validation coverage and its limits

Unlike API version 0, whose final accept/reject verdict comes from a
vendored, byte-identical copy of xahaud's own checker compiled into
`hooks-build` (`docs/DESIGN.md` §6.5 — `Guard.h`, kept in
`crates/hooks-build/vendor/`, CI-verified against upstream), **API version
1 has no vendored counterpart in this repo**. `GasValidator.cpp` (the
xahaud source that actually decides whether a Gas-type wasm is SetHook-
legal) lives on a `gas-hook` feature branch of `Xahau/xahaud` that has not
merged to `release` — there is nothing byte-identical to vendor yet, and no
`-DGUARD_CHECKER_BUILD`-style standalone build mode was found for it the way
`Guard.h` has.

The rules `hooks-build`'s Rust validator enforces for `--api-version 1`
(`crates/hooks-build/src/validator.rs`) were written by reading that
branch's `GasValidator.cpp`/`GasValidator.h` directly (not vendored — no
`SHA256SUMS` tripwire, no CI vendor-sync check) and porting its checks by
hand:

- Export section (`validateExportSection`): only `hook`/`cbak`/`__`-prefixed
  function exports allowed; `hook`/`cbak` signature must be `(i32) -> i64`;
  an exported memory's minimum and maximum page counts must each be ≤ 8
  (`hook_api::max_memory_pages`).
- Import section (`validateImportSection`): only function imports from
  module `env`; `_g` is forbidden outright; every import must match the
  Hook API whitelist's signature exactly.

**What this means in practice**: a module `hooks-build check --api-version
1` accepts has no independent second opinion the way a v0 module does — if
the hand-ported rules above have a gap relative to the real
`GasValidator.cpp` (for example, the whitelist gating by `Rules`/active
amendments that `hook_api::getImportWhitelist(rules)` performs, which this
port does not replicate — `crates/hooks-build/src/whitelist.rs` is a flat,
unconditional table), `hooks-build` could accept a module that real SetHook
validation on a `HookGas`-enabled node would reject, or vice versa. Treat
`hooks-build check --api-version 1`'s verdict as informative, not
authoritative, until:

1. `GasValidator.cpp` merges to `Xahau/xahaud`'s `release` branch and can be
   vendored the same way `Guard.h` is (see `docs/DESIGN.md` §6.5 and
   `scripts/sync-vendor.sh`), and
2. it is confirmed to build standalone (or a shim is written) so it can be
   compiled into `hooks-build` as the authoritative v1 verdict.

Until then, any Gas-type hook intended for real deployment should be
verified against an actual `HookGas`-enabled node before relying on
`hooks-build`'s verdict alone.

## Network prerequisite

Gas-type hooks require the `HookGas` amendment. As of this writing (see
`mise.toml`'s pinned `XAHAUD_VERSION`) that amendment is not active on
Xahau mainnet or testnet, and this repo's e2e suite
(`docs/E2E-TESTING.md`) has not exercised a Gas-type hook against a live
node — `examples/gas-counter` is verified at the `hooks-build build`/`check`
level only (see "Validation coverage and its limits" above for why that
verdict is weaker than the v0 examples'). Attempting to SetHook a
Gas-type hook (`sfHookApiVersion = 1`) against a node without `HookGas`
enabled fails with `temMALFORMED` (`sfHookGas present but featureHookGas
disabled` — see the `gas-hook-dev` skill's error table for the full list).

## v0 vs v1 at a glance

| | Guard-type (v0) | Gas-type (v1) |
|---|---|---|
| Loop bound proof | Static: `guard!`/`guard_m!`, verified by the vendored checker at SetHook time | Runtime: gas pool metering (`sfHookGas`) |
| `hooks-build` passes run | clean → flatten → unnest → guard → validate | clean → validate (flatten/unnest/guard skipped) |
| `_g` import | Required (even with zero loops — R1, §6.2b) | Forbidden |
| Exported memory | Never seen in practice (cleaner strips it; a surviving one is a hard error) | Allowed, capped at 8 pages (min and max) |
| `__`-prefixed function exports | Hard error (only `hook`/`cbak` allowed) | Allowed |
| Fee/cost model | SetHook fee from static worst-case instruction count | SetHook fee unchanged; **plus** `sfHookGas`/`sfHookCallbackGas`/`sfHookWeakGas` charged per triggering transaction (1 drop/unit) |
| `hooks-build`'s verdict authority | Vendored, byte-identical upstream checker (authoritative) | Hand-ported Rust rules only (informative — see limits above) |
| Example | `examples/state-counter` | `examples/gas-counter` |

## Measured size difference: `state-counter` vs `gas-counter`

Current-toolchain `hooks-build build` output (exact byte counts drift with
compiler versions; these are illustrative, not a guarantee):

| | `state-counter` (v0) | `gas-counter` (v1) |
|---|---|---|
| Size | 374 bytes | 452 bytes |
| Estimated SetHook fee | 1,870,000 drops | 2,260,000 drops |

`gas-counter` is **larger**, not smaller. This is not a contradiction: the
two hooks are not the same logic — `gas-counter` deliberately does strictly
more than `state-counter` (an extra `for` loop over a 4-element step table,
plus an 8-byte array-equality check against a reset marker) specifically to
demonstrate that ordinary Rust control flow needs no guard annotation under
v1, and that extra logic's own code and data outweighs whatever bytes
skipping flatten saves.

For an apples-to-apples measurement of the pipeline difference alone
(same source, same logic, only `--api-version` differs), `state-counter`'s
own crate was built both ways:

| | `--api-version 0` (as shipped) | `--api-version 1` |
|---|---|---|
| Size | 374 bytes | 376 bytes |
| Estimated SetHook fee | 1,870,000 drops | 1,880,000 drops |
| Max nesting depth (reported) | 1 (post-unnest) | 2 (unnest skipped) |

The two are within 2 bytes of each other — for a hook this small, flatten's
whole-module inlining has almost nothing to inline (the hook body already
makes one call each to `state`/`state_set`/`accept`/`rollback`, and LLVM's
own inlining already flattened most of that at `-O`), so skipping it costs
essentially nothing either way. The visible pipeline difference here is
structural, not size: nesting depth is reported *before* unnest's ladder
collapse for `--api-version 1` (unnest is v0-only, since it exists purely
to satisfy the v0 guard checker's 32-level nesting limit — `docs/DESIGN.md`
§6.2c), so the same source reports depth 1 under v0 (post-collapse) and
depth 2 under v1 (uncollapsed) — neither is a hard error at this depth
either way. Flatten/unnest's savings (or cost, since `flatten` can also
*add* bytes by duplicating shared helpers at every call site — see
`docs/DESIGN.md` §6.2b) would show up more clearly on a hook with a real
call graph (shared helpers, multiple call sites) — none of the existing
examples are big enough for that difference to be dramatic.

## What did *not* change in this pass

- `hooks-lib` gained no Gas-type-specific API, macro, or feature flag — the
  existing wrapper works as-is for v1 because none of its internals call
  `guard!`/`guard_m!`. `docs/DESIGN.md`'s "Non-goals" note ("hooks-lib v1
  targets Guard-type hooks... Gas-type ergonomics" is out of scope) still
  stands as written; this pass is about `hooks-build`'s v1 *validation*
  correctness and a first example, not new v1-specific ergonomics.
- No amendment-aware whitelist gating (`Rules`-dependent
  `getImportWhitelist`) — `whitelist.rs` remains a flat table for both
  api versions.
- No e2e (live-node) coverage for Gas-type hooks — see "Network
  prerequisite" above.
