# The `rshooks` CLI

`rshooks` is a single binary with three subcommands: `build` (the
one you'll use for everyday work), `clean` (post-process an
already-compiled wasm without invoking cargo), and `check` (validate any
wasm file against the full SetHook rule set, without modifying it). This
page is the complete flag reference for all three, taken directly from the
CLI's own definitions.

Every subcommand also accepts the standard clap-generated `-h`/`--help`;
`rshooks --version` prints the installed version.

## `rshooks build`

Builds a Rust crate for `wasm32v1-none` (`cargo build --release --target
wasm32v1-none`), then cleans and validates the result into a SetHook-legal
binary. This is the pipeline described in [Building a Hook](../getting-started/building.md).

```sh
rshooks build --manifest-path path/to/Cargo.toml
```

| flag | default | description |
|---|---|---|
| `--manifest-path <PATH>` | cargo's default (current directory) | Path to the crate's `Cargo.toml`, forwarded to `cargo build`. |
| `-p, --package <NAME>` | none | Build only the named package, forwarded to `cargo build -p`. Useful when `--manifest-path` points at a workspace. |
| `--api-version <0\|1>` | `0` | The Hook API version this module targets. `0` is Guard-type (loop guards required); `1` is Gas-type (guard handling skipped). |
| `--auto-guard` | off | Insert missing loop guards instead of treating an unguarded loop as a build error. |
| `--default-maxiter <N>` | `16` | The `maxiter` value used for auto-inserted guards, when `--auto-guard` is set. |
| `--out <DIR>` | `out/` next to the manifest | Directory to write the output binary (and metadata sidecar, if any) to. |
| `--allow-oversize` | off | Write the output even if it exceeds the 65,535-byte SetHook size limit. The result is still clearly marked invalid in the printed report. |

On success, `build` writes `out/<crate>.wasm` (matching cargo's own
artifact file name) and, if the crate declares `metadata!`, a matching
`out/<crate>.json` sidecar — see [Hook Metadata](metadata.md). A stale
sidecar from a previous build that no longer declares `metadata!` is
removed automatically.

## `rshooks clean`

Cleans and validates an already-built wasm file directly, without invoking
cargo. Useful for post-processing an artifact you already have on disk —
for example one built by a different pipeline, or one you want to
reprocess with different flags without rebuilding.

```sh
rshooks clean path/to/artifact.wasm
```

| flag | default | description |
|---|---|---|
| `input` (positional) | — | The input wasm file. Required. |
| `-o, --out <PATH>` | `<input>.clean.wasm` | Where to write the cleaned binary. |
| `--api-version <0\|1>` | `0` | The Hook API version this module targets. |
| `--auto-guard` | off | Insert missing loop guards instead of treating them as an error. |
| `--default-maxiter <N>` | `16` | `maxiter` used for auto-inserted guards. |
| `--allow-oversize` | off | Write the output even if it exceeds the 65,535-byte SetHook limit. |

`clean` does not generate a metadata sidecar — that step is specific to
`build`, since it needs the original crate's `metadata!` carrier from
cargo's raw artifact.

## `rshooks check`

Validates a wasm file against the full SetHook rule set without modifying
it. Unlike `build`/`clean`, this works on **any** wasm file, including
ones not built by this toolchain at all — for example, a Hook compiled
from C.

```sh
rshooks check path/to/hook.wasm
```

| flag | default | description |
|---|---|---|
| `file` (positional) | — | The wasm file to validate. Required. |
| `--api-version <0\|1>` | `0` | The Hook API version this module targets. |

On success, `check` prints the same worst-case-instruction and
nesting-depth report as `build`/`clean`, followed by `OK: <file> is a
valid SetHook wasm binary` and the size/fee estimate. On failure, it
prints `INVALID: <file> failed validation:` with the specific reasons, and
exits with a non-zero status — making it suitable for a CI gate on hand-
written or third-party wasm as well as this toolchain's own output.
