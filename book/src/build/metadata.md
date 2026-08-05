# Hook Metadata

A Hook crate can declare a `metadata!` block describing itself — its
name, description, trigger transaction types, and on-ledger `HookName`.
`rshooks-build build` reads this declaration and writes a JSON sidecar
next to the compiled wasm, combining what you wrote with facts only
available after the build (the binary's hash and its worst-case
instruction counts). This page covers the full grammar and the sidecar's
exact shape.

## Declaring metadata

```rust
use rshooks::metadata;

metadata! {
    name: "payment observer",
    description: "Observes incoming and outgoing payments.",
    HookOn: [Payment, Invoke],
    HookCanEmit: [Payment],
    HookName: "pay-hook",
}
```

- **`name`** — required, a non-empty display name for the Hook.
- **`description`** — optional, free-form text.
- **`HookOn`** — optional, a list of bare `TxType` variant names (see
  below) that trigger this Hook in both directions.
- **`HookCanEmit`** — optional, the transaction types this Hook declares
  it may emit.
- **`HookName`** — optional, the UTF-8 string placed in SetHook's
  `HookName` field.

At most one `metadata!` declaration should appear in a Hook crate. It can
sit anywhere at module scope in `src/lib.rs` — it doesn't need to be
referenced by `hook` or `cbak` to take effect.

## Directional triggers: `IncomingHookOn` / `OutgoingHookOn`

`HookOn` is mutually exclusive with a directional form, where both arrays
are required together:

```rust
metadata! {
    name: "directional hook",
    IncomingHookOn: [Payment, Invoke],
    OutgoingHookOn: [Payment],
}
```

A crate may declare `HookOn` alone, `IncomingHookOn` **and**
`OutgoingHookOn` together, or omit all three trigger fields entirely — any
other combination (for example `HookOn` alongside `IncomingHookOn`, or
`IncomingHookOn` without `OutgoingHookOn`) is rejected at build time.
`IncomingHookOn` and `OutgoingHookOn` must also describe genuinely
different sets of transaction types; if they'd end up identical, the build
rejects it and asks you to use plain `HookOn` instead, since that's what
it means. When all three are omitted, the sidecar represents the
resulting all-zero raw `HookOn` value as `null`.

## Transaction type names

Every entry in `HookOn`, `IncomingHookOn`, `OutgoingHookOn`, and
`HookCanEmit` is a bare [`TxType`](../reference/raw.md) variant name —
`Payment`, not `TxType::Payment` or `ttPAYMENT`. Because the macro
resolves each name against the real enum, a misspelling is a compile
error, not a silent no-op. Duplicate entries within one list are also
rejected.

Names use Xahau's canonical `TransactionType` spellings, including some
that are easy to get wrong by guessing: `SetHook`, `SetRegularKey`, and
`AMMCreate`. `rshooks-build` maintains the authoritative list of every
valid name against the actual protocol transaction set.

## `HookName`

`HookName` is a Rust UTF-8 string. The macro itself enforces **2 through 8
Unicode scalar values** — deliberately counting characters, not encoded
bytes. This is a separate rule from xahaud's own ledger-level requirement
that a `HookName` be **4 through 16 UTF-8 bytes**; a name intended for
direct on-chain submission needs to satisfy both. Because these two rules
can diverge for non-ASCII names, `rshooks-build build` checks the
byte-length rule too and prints a warning (not a hard error) when a
declared `HookName` doesn't fit it.

## How metadata travels through the build

`metadata!`'s expansion carries the declaration as compact JSON, hex-
encoded into the name of a Hook wasm export that is never actually called
— a deliberately dead export named `__rshooks_metadata_v1_<HEX>`.
`rshooks-build build` reads this carrier from cargo's *raw* artifact,
before cleaning, and the ordinary hook-cleaner pass then removes it along
with every other non-`hook`/`cbak` export. **The declaration is build-only
and never changes the deployed binary**: it adds no data segment, no
runtime code, no import, and no byte to the final wasm — the same
`HookHash` and WCE would result whether or not `metadata!` was present at
all.

## The JSON sidecar

For a Hook declaring:

```rust
metadata! {
    name: "accept-all",
    description: "Accepts every transaction selected by HookOn.",
    HookOn: [Invoke],
    HookName: "accept",
}
```

`rshooks-build build` writes an `out/<crate>.json` sidecar shaped like
this (real output, from the `accept-all` example):

```json
{
  "name": "accept-all",
  "description": "Accepts every transaction selected by HookOn.",
  "HookOn": "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF7FFFFFFFFFFFFFFFFFFBFFFFF",
  "HookCanEmit": null,
  "HookName": "616363657074",
  "HookHash": "DCE6A3F81224AE89C557F04D73420D808D9009BCF1CFC1474396CD2DA2D4DF16",
  "WCE": {
    "hook": 15,
    "cbak": 0
  },
  "human": {
    "HookOn": [
      "Invoke"
    ],
    "HookCanEmit": null,
    "HookName": "accept"
  }
}
```

- **Top-level `HookOn`/`HookOnIncoming`/`HookOnOutgoing`** — the raw,
  deployable SetHook value: a 32-byte hex string encoding Xahau's inverted
  transaction-type bitmask (every bit set except the ones corresponding to
  the transaction types you listed). This is the exact bytes a `SetHook`
  transaction's `HookOn` field expects; `null` when no trigger fields were
  declared. `HookOnIncoming`/`HookOnOutgoing` appear instead of `HookOn`
  when the source used the directional form.
- **`HookCanEmit`** — the same bitmask encoding, or `null` if omitted.
- **`HookName`** — the declared name's raw UTF-8 bytes as uppercase hex
  (`"accept"` → `616363657074`), matching what `SetHook` expects on the
  wire. `null` if `HookName` wasn't declared.
- **`HookHash`** — Xahau's hash of the deployed binary: the uppercase hex
  of the first 32 bytes of the final cleaned wasm's SHA-512 digest. This
  identifies the exact Hook code, independent of which account installs
  it.
- **`WCE`** — the same worst-case-execution figures printed to the
  terminal during the build: static instruction-count upper bounds for
  `hook` and `cbak`, or `null` for both on a Gas-type (`--api-version 1`)
  module, which has no static bound of this kind.
- **`human`** — the readable, source-level form of every field above:
  transaction type names as written, and the `HookName` string itself
  rather than its hex encoding. Use `human` to review what a sidecar
  declares; use the top-level fields when constructing an actual
  `SetHook` transaction.

Two consistency checks run at build time and surface as warnings (not hard
errors) in the sidecar generation step: declaring `HookCanEmit` when the
final wasm never actually calls the `emit` API, and calling `emit` without
having declared `HookCanEmit` at all.
