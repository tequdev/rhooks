# gas-counter

A HookApiVersion 1 ("Gas"-type) counterpart to [`state-counter`](../02_state-counter):
the same persistent-counter behavior (`state`/`state_set` round-trip,
zero-padded 32-byte state key), but demonstrating what changes once loop
guards are no longer required.

## What's different from `state-counter`

- Built with `--api-version 1` instead of the toolchain default (`0`).
- Increments the counter by summing a fixed step table with an **ordinary
  Rust `for` loop** — no `guard!`/`guard_m!` call anywhere in the source.
- Detects a sentinel "reset" state value with a **plain `[u8; 8]` equality
  check** — no `--auto-guard` build flag needed, even though this is exactly
  the kind of comparison that LLVM can lower to an unguarded byte-compare
  loop at `opt-level = "z"` (see `firewall`'s `AccountId` comparison and its
  README).

Under a Guard-type (v0) build, the `for` loop above would need an explicit
`guard!(STEPS.len() as u64)` (or `--auto-guard`) before `hooks-build build`
would accept the binary at all — see `docs/DESIGN.md` §6.3. Under Gas-type
(v1), the guard pass is skipped entirely: loop iteration is bounded at
runtime by the transaction's pre-allocated gas pool (`sfHookGas`) instead of
a static instruction-count analysis, so ordinary Rust control flow needs no
extra annotation. See `docs/GAS-HOOKS.md` for the full v0-vs-v1 comparison.

## Build

```sh
cargo run -p hooks-build -- build --manifest-path examples/11_gas-counter/Cargo.toml --api-version 1
```

`--api-version 1` is required on both `build` and `check` — omitting it
builds/validates against the Guard-type (v0) rule set instead, which this
hook does not satisfy (it has no `_g` import and an unguarded loop).

## Prerequisite for on-chain use

Gas-type hooks (HookApiVersion 1) require the `HookGas` amendment to be
enabled on the target network. As of this writing that amendment is not yet
active on Xahau mainnet or testnet; this example builds and validates
locally but has not been exercised against a live node (see
`docs/GAS-HOOKS.md`).
