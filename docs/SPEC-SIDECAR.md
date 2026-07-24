# Interface-spec sidecar & TypeScript bindings

Complements `docs/DESIGN.md` §6 (`hooks-build`). This document covers two
`hooks-build` features not yet folded into DESIGN.md's numbered sections:
the `spec.json` sidecar file every `build` produces, and the `bindings-ts`
subcommand that turns one into a TypeScript module.

## Why a sidecar, not a custom wasm section

`soroban` embeds its contract spec in a wasm custom section. `hooks-build`
cannot do that: the cleaner (`docs/DESIGN.md` §6.2) strips every custom
section unconditionally, because SetHook's 65,535-byte limit (5000 drops of
XAH per byte) makes every byte of a shipped hook expensive, and interface
documentation has no business costing the account that installs the hook
anything. Instead, `hooks-build build` writes a **sidecar JSON file** next
to the binary: `out/<name>.spec.json`. It never enters the wasm; it exists
purely for tooling (docs generators, TypeScript bindings, deployment
scripts) that runs outside the hook.

## `hook-spec.toml` (optional input)

Place a `hook-spec.toml` next to a hook crate's `Cargo.toml`. It is entirely
optional — `hooks-build build` always writes a `spec.json`, containing at
minimum the build facts below, whether or not this file exists.

```toml
# NOTE on field order: TOML's bare `key = value` lines belong to whichever
# table header preceded them. Root-level fields (`transaction_types`) MUST
# appear *before* the first `[hook]`/`[invoke]` table header, or they
# silently become ignored extra fields on that table instead of landing on
# the document root.

transaction_types = ["Payment"]   # TransactionTypes this hook expects (informational)

[hook]
name = "firewall"                 # conventionally the crate name
description = "..."                # one paragraph

[invoke]
hook_on = "Payment"                 # free-text HookOn summary (informational)
notes = "..."                        # any other invocation caveats

[[params]]                          # one entry per HookParameter
name = "BL"
bytes = 20                          # expected HookParameterValue length
description = "20-byte blocked AccountId."

[[state]]                           # one entry per hook-state key this hook reads/writes
key_name = "counter"                # short unique name, documentation only
key_derivation = "ascii_padded"     # "ascii_padded" | "hex" — see below
key = "counter"                     # ASCII literal or 32-byte hex, per key_derivation
value_type = "u64"                  # informational (u64, u32, account_id, bytes, ...)
value_bytes = 8
description = "Monotonic invocation counter."
```

`key_derivation`:
- `ascii_padded` — `key` is an ASCII literal, right-zero-padded to 32 bytes.
  This is exactly `hooks_lib::pad!(key.as_bytes())` (see
  `examples/state-counter`'s `STATE_KEY`) — the common idiom for a
  human-readable state-key name.
- `hex` — `key` is already the full 32-byte state key, hex-encoded (for
  keys derived some other way, e.g. from an account ID or a hash).

Every field documented here is the complete schema — see
`crates/hooks-build/src/spec.rs` for the authoritative serde types
(`HookSpecInput`, `HookMeta`, `ParamSpec`, `StateKeySpec`, `KeyDerivation`,
`InvokeSpec`); this file is generated documentation of that source, kept in
sync by review, not by a codegen step.

## `spec.json` (merged output)

Written by `hooks-build build` (not `clean`/`check`) to
`<out-dir>/<crate-name>.spec.json`, always, merging `hook-spec.toml` (if
found) with build facts gathered during that build:

```json
{
  "spec_version": 1,
  "hook": { "name": "firewall", "description": "..." },
  "params": [{ "name": "BL", "bytes": 20, "description": "..." }],
  "state": [
    {
      "key_name": "counter",
      "key_derivation": "ascii_padded",
      "key": "counter",
      "value_type": "u64",
      "value_bytes": 8,
      "description": "..."
    }
  ],
  "transaction_types": ["Payment"],
  "invoke": { "hook_on": "Payment", "notes": "..." },
  "build": {
    "wasm_file": "firewall.wasm",
    "size_bytes": 523,
    "sha256": "<lowercase hex, 64 chars>",
    "hook_api_version": 0,
    "hook_cost": 714,
    "cbak_cost": 0,
    "sdk_version": "0.1.0"
  }
}
```

- `hook`/`params`/`state`/`transaction_types`/`invoke` are omitted entirely
  (not emitted as empty/null) when no `hook-spec.toml` was found — the
  minimal spec.json in that case is just `{"spec_version": 1, "build": {...}}`.
- `build` is always present:
  - `hook_cost`/`cbak_cost` are the vendored upstream guard checker's
    worst-case instruction counts (`docs/DESIGN.md` §6.5) — the same
    numbers `build` prints to stdout. `null` for API version 1 (no guard
    checker runs) or if no verdict was attached.
  - `sdk_version` is the `hooks-build` crate version (`CARGO_PKG_VERSION`)
    that produced the artifact.
  - `sha256` is over the final output bytes (post-clean/flatten/unnest/
    guard), matching exactly what would be installed via `SetHook`.

## `bindings-ts`: generating TypeScript bindings

```sh
hooks-build bindings-ts <spec.json> --out <dir>
```

Reads a `spec.json` and writes `<dir>/<hook-name>.ts` — a small,
dependency-free TypeScript module meant to replace hand-written hex in
`@transia/hooks-toolkit`-based e2e tests (`e2e/test/*.test.ts` today write
`convertStringToHex('BL')` and manually zero-pad/hex-encode state keys by
hand). Nothing in the generator is toolkit-specific beyond the
`HookParameters` array shape (`{ HookParameter: { HookParameterName,
HookParameterValue } }`), which is the `SetHook` transaction's own wire
shape.

Generated per non-empty section of the spec:

| spec input | generated TS |
|---|---|
| `params` | `HOOK_PARAM_NAMES` (hex-encoded, uppercase, no `0x`), `HOOK_PARAM_BYTES` (expected lengths), a `<PascalName>HookParameterValues` type, and `build<PascalName>HookParameters(values)` — validates each value's decoded byte length and assembles the `HookParameters` array |
| `state` | `HOOK_STATE_KEYS` — each entry's 32-byte key, pre-computed and hex-encoded (an `ascii_padded` entry is padded here, at generation time; the consumer never reimplements `pad!`) |
| `transaction_types` | `HOOK_TRANSACTION_TYPES` (a `const` string-literal tuple) |
| `hook_api_version` | `HOOK_API_VERSION` |
| `build` | `HOOK_BUILD` (`wasmFile`, `sizeBytes`, `sha256`, `hookApiVersion`, `hookCost`, `cbakCost`, `sdkVersion`) — always emitted |

A section backed by empty/absent data (no `hook-spec.toml`, or one that
declared no params/state/transaction types) is omitted from the output
entirely, not emitted as an empty or misleading constant.

The generated file starts with an `AUTO-GENERATED ... do not edit by hand`
comment naming its `spec.json` source. Regenerate by rerunning
`bindings-ts`, never by hand-editing.

## Demo: `mise run spec-bindings`

```sh
mise run spec-bindings
```

Builds `examples/firewall` (which ships a `hook-spec.toml` documenting its
`BL` parameter), generates TypeScript bindings from the resulting
`out/firewall.spec.json` into `e2e/generated/firewall.ts`, and runs
`pnpm --dir e2e exec tsc --noEmit` to confirm the generated module actually
compiles alongside the rest of `e2e/`. `e2e/generated/` is gitignored — a
build artifact, regenerated on demand, not checked in.

This does **not** run the e2e test suite itself (that needs a live
standalone node via `mise run e2e:node-up`, `docs/E2E-TESTING.md`) and does
**not** rewire `e2e/test/firewall.test.ts` to import the generated module —
that file is owned by the e2e test lane; adopting the generated bindings
there (replacing `convertStringToHex('BL')` with
`HOOK_PARAM_NAMES.BL`, and `accountIdHex(...)` plus the manual
`HookParameters` array with `buildFirewallHookParameters({ BL: ... })`) is a
follow-up for whoever owns that file next.
