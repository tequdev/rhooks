# Building a Hook

The previous chapter ran `rshooks build` without explaining what it
actually does. This chapter walks through the pipeline stage by stage, so
the printed report and the `check` subcommand make sense on their own.

## The pipeline

`rshooks build` runs cargo, then a fixed sequence of post-processing
and validation steps on the resulting `.wasm`:

1. **`cargo build --release --target wasm32v1-none`** — compiles your
   crate exactly as any other Rust crate would be, using your `Cargo.toml`
   and its `[profile.release]` settings (see [Installation](installation.md)
   for why that profile matters). The output is an ordinary `cdylib`
   artifact; on its own it is **not** SetHook-valid — it still exports
   `memory`, and Rust's own code generation gives no guarantee about loop
   guards or WASM feature usage.
2. **Hook-cleaner** — strips the disallowed `memory` export and any other
   dead or non-`hook`/`cbak` exports, and (for Guard-type, API version 0,
   modules) flattens and inlines the crate's call graph into the `hook`/
   `cbak` entry points, untangling the resulting block/loop/if nesting so
   it fits the host's structural limits. This is also the stage that
   strips a `metadata!` declaration's carrier export — see [Hook
   Metadata](../build/metadata.md) for why that carrier never reaches the
   deployable binary.
3. **Guard checker** — for API version 0, validates that every loop begins
   with the exact guard call sequence the host requires, and computes the
   static worst-case instruction count (WCE) for `hook` and, if present,
   `cbak`, from those guards. This step is skipped for API version 1
   (Gas-type hooks meter instructions at runtime instead of requiring
   static guards).
4. **Validator** — checks the complete SetHook rule set: exactly one
   `hook` export (and at most one `cbak`), no disallowed imports, no
   recursion, and a binary size at or under the 65,535-byte SetHook limit
   (unless `--allow-oversize` is passed, in which case the output is still
   written but clearly marked invalid).
5. **Metadata sidecar** — if the crate declared `metadata!`, writes the
   `<crate>.json` sidecar next to the cleaned wasm, combining the source
   declaration with the final binary's `HookHash` and WCE.

Every one of these steps runs against the exact bytes that will be
deployed — the printed WCE and `HookHash` describe the file actually
written to `out/`, not an intermediate artifact.

## Reading the printed report

A successful `build` prints, in order:

```text
worst-case instructions: hook=15 cbak=0
max nesting depth: 0
wrote out/my_hook.wasm
size: 174 bytes
estimated SetHook fee: 870000 drops (0.870000 XAH)
wrote out/my_hook.json
```

- **`worst-case instructions`** is the guard checker's static upper bound
  on instructions the host will ever execute for each entry point. It only
  appears for API version 0 (Guard-type) modules — a Gas-type module has no
  static bound of this kind.
- **`max nesting depth`** is the deepest block/loop/if nesting in the final
  module, checked against the host's structural limit.
- **`size` / `estimated SetHook fee`** are computed directly from the final
  binary's byte count — SetHook's fee schedule is `bytes × 5000` drops, so
  this is the actual one-time deployment fee cost of the binary you just
  built, not an approximation.

## Validating a binary without building it

`rshooks check <file>` runs the same guard-checker and validator
steps against an existing wasm file, without invoking cargo or writing any
output. It works on any SetHook-shaped wasm, including one this toolchain
didn't build — see [The `rshooks` CLI](../build/cli.md) for its full
flag reference, and [`build`](../build/cli.md)'s and [`clean`](../build/cli.md)'s
as well.

## A note on `--auto-guard`

Guards are your responsibility by default: an unguarded loop is treated as
a hard build error, on the principle that a missing `guard!` in your own
source is a bug, not something the toolchain should paper over. The
`--auto-guard` flag exists mainly for loops the *compiler* generates that
never appear in your Rust source at all (certain array-equality and
buffer-zeroing patterns can lower to an unguarded loop at the WASM level).
It's covered in full, including why it's a footgun if used carelessly and
the source-level idioms that usually avoid needing it in the first place,
in the [Guards and Loops](../concepts/guards.md) chapter.
