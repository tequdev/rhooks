# firewall

Reads the originating transaction's sender (`otxn_field(sfAccount)`) and a
Hook parameter named `BL` (the blocked `AccountId`, 20 bytes). Rolls the
transaction back if they match; accepts otherwise (including when `BL`
isn't configured — nothing to block). Straight-line code: no loop is
written in the source, so no `guard!` is needed there.

## Build

```sh
cargo run -p hooks-build -- build --manifest-path examples/firewall/Cargo.toml \
  --auto-guard --default-maxiter 24
```

`--auto-guard` **is** required here even though the source has no loop:
`sender == blocked` (`[u8; 20]` equality) compiles to a compiler-generated
byte-compare loop under `opt-level = "z"` on `wasm32v1-none` (no
bulk-memory instructions to do it inline). `--default-maxiter 24` is used
instead of the `--auto-guard` default of 16 because the loop's true worst
case is 20 iterations (one per `AccountId` byte) — 16 would build
successfully but risk a runtime `GUARD_VIOLATION` on a real node. See
`examples/README.md`'s "On `--auto-guard`" section for the full story.

## Configuring the blacklist

Set a `BL` Hook parameter (20 raw bytes, the blocked `AccountId`) when
installing this Hook via `SetHook`. Deployment/SetHook tooling is out of
scope for this repo (see `docs/DESIGN.md` §1 non-goals).

## Interface spec sidecar & TypeScript bindings

This example ships a hand-authored `hook-spec.toml` documenting its
interface (the `BL` parameter, expected `TransactionType`s, invocation
notes). `hooks-build build` merges it with the build output into
`out/firewall.spec.json` automatically. `mise run spec-bindings` builds this
example and generates a TypeScript module from that spec.json into
`e2e/generated/firewall.ts` — see `docs/SPEC-SIDECAR.md` for the full
schema and what the generated bindings look like.
