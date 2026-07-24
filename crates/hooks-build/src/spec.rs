//! Interface-spec sidecar (`docs/SPEC-SIDECAR.md`).
//!
//! `soroban` embeds its contract spec in a wasm custom section; `hooks-build`
//! cannot do that here because the cleaner (`crate::cleaner`) strips every
//! custom section unconditionally (§6.2 — SetHook's 65,535-byte limit makes
//! every byte of a shipped hook expensive, and a spec has no business
//! costing the account that installs the hook anything). Instead the spec is
//! a **sidecar JSON file** written next to the binary: `out/<name>.spec.json`.
//!
//! Two inputs are merged into that file:
//!
//! - **`hook-spec.toml`** (optional, hand-authored, lives next to the hook
//!   crate's `Cargo.toml`): human-supplied interface documentation — hook
//!   name/description, Hook parameters, hook-state key schema, expected
//!   `TransactionType`s, and invocation notes. See [`HookSpecInput`].
//! - **Build facts** (always present, gathered by `hooks-build build`
//!   itself): the output wasm's file name/size/sha256, the `HookApiVersion`,
//!   the vendored guard checker's worst-case instruction counts (when
//!   available — API version 0 only), and the `hooks-build` version that
//!   produced the artifact. See [`BuildInfo`].
//!
//! If `hook-spec.toml` is absent, `hooks-build build` still always writes a
//! minimal `spec.json` containing only [`BuildInfo`] — every build gets a
//! sidecar, hand-authored interface documentation is opt-in on top of that.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::ApiVersion;
use crate::guard_native::GuardVerdict;

/// The `spec.json` schema version. Bump only on a breaking change to the
/// output shape (consumers, e.g. `bindings-ts`, may switch on this).
pub const SPEC_VERSION: u32 = 1;

/// Hand-authored `hook-spec.toml`, read from next to the hook crate's
/// `Cargo.toml` if present. Every field is optional at the TOML level except
/// `[hook]` itself — a file that exists is assumed to at least name the
/// hook.
///
/// `deny_unknown_fields` here and on every nested type below is deliberate:
/// TOML's bare `key = value` lines attach to whichever table header
/// preceded them, so a misplaced root-level field (e.g. `transaction_types`
/// written *after* `[hook]` instead of before it) silently becomes an
/// ignored extra field on the wrong table under the default lenient
/// behavior — exactly the kind of mistake this schema should catch loudly
/// at parse time instead of dropping data silently.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookSpecInput {
    /// Hook identity.
    pub hook: HookMeta,
    /// Hook parameters (`HookParameters` in a `SetHook` transaction).
    #[serde(default)]
    pub params: Vec<ParamSpec>,
    /// Hook-state key schema.
    #[serde(default)]
    pub state: Vec<StateKeySpec>,
    /// `TransactionType`s this hook expects to be invoked for (informational
    /// — not enforced; corresponds to the `HookOn` bitmask a deployer would
    /// configure).
    #[serde(default)]
    pub transaction_types: Vec<String>,
    /// Free-form notes on invocation conditions (e.g. `HookOn` bitmask
    /// caveats, weak/strong execution, namespace expectations).
    #[serde(default)]
    pub invoke: Option<InvokeSpec>,
}

/// Hook name and human description, `[hook]` in `hook-spec.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookMeta {
    /// The hook's name (conventionally the crate name).
    pub name: String,
    /// A one-paragraph description of what the hook does.
    #[serde(default)]
    pub description: String,
}

/// One `HookParameter` the hook expects, `[[params]]` in `hook-spec.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamSpec {
    /// The parameter's name (`HookParameterName`, before hex-encoding).
    pub name: String,
    /// The expected length in bytes of `HookParameterValue`.
    pub bytes: u32,
    /// A human description of the parameter's meaning.
    #[serde(default)]
    pub description: String,
}

/// How a hook-state key is derived from [`StateKeySpec::key`], `[[state]]`
/// in `hook-spec.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyDerivation {
    /// `key` is an ASCII literal, zero-padded on the right to 32 bytes —
    /// exactly `hooks_lib::pad!(key.as_bytes())` (see `state-counter`).
    AsciiPadded,
    /// `key` is already the full 32-byte state key, hex-encoded.
    Hex,
}

/// One hook-state entry's key schema, `[[state]]` in `hook-spec.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateKeySpec {
    /// A short, unique name for this state entry (documentation only).
    pub key_name: String,
    /// How `key` is turned into the 32-byte state key.
    pub key_derivation: KeyDerivation,
    /// The key source: an ASCII literal (`ascii_padded`) or a 32-byte hex
    /// string (`hex`), per `key_derivation`.
    pub key: String,
    /// The stored value's logical type (e.g. `u64`, `u32`, `account_id`,
    /// `bytes`) — informational, not enforced.
    pub value_type: String,
    /// The stored value's length in bytes.
    pub value_bytes: u32,
    /// A human description of what the value means.
    #[serde(default)]
    pub description: String,
}

/// Invocation notes, `[invoke]` in `hook-spec.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvokeSpec {
    /// The `HookOn` transaction-type list this hook is intended to be
    /// installed with (informational; mirrors `transaction_types` but as
    /// free text when a precise list isn't meaningful, e.g. "all but
    /// HookSet").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_on: Option<String>,
    /// Any other invocation caveats (namespace expectations, weak vs. strong
    /// execution, `cbak`-only behavior, etc).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Build facts gathered by `hooks-build build`, always present in
/// `spec.json` regardless of whether `hook-spec.toml` exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildInfo {
    /// The output wasm's file name (e.g. `firewall.wasm`).
    pub wasm_file: String,
    /// The output wasm's size in bytes.
    pub size_bytes: u64,
    /// The output wasm's SHA-256, lowercase hex.
    pub sha256: String,
    /// The `HookApiVersion` this module targets (0 or 1).
    pub hook_api_version: u8,
    /// The vendored guard checker's worst-case instruction count for
    /// `hook()`. `None` for API version 1 (no guard checker runs) or if
    /// `check`/`verify` did not attach a verdict.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_cost: Option<u64>,
    /// The vendored guard checker's worst-case instruction count for
    /// `cbak()` (0 if the module has no `cbak` export). `None` under the
    /// same conditions as `hook_cost`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cbak_cost: Option<u64>,
    /// The `hooks-build` version that produced this artifact
    /// (`CARGO_PKG_VERSION`).
    pub sdk_version: String,
}

/// The full merged `spec.json` document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookSpec {
    /// Schema version, see [`SPEC_VERSION`].
    pub spec_version: u32,
    /// Hook identity, `None` when no `hook-spec.toml` was found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook: Option<HookMeta>,
    /// Hook parameters; empty when no `hook-spec.toml` was found or it
    /// declared none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<ParamSpec>,
    /// Hook-state key schema; empty under the same conditions as `params`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state: Vec<StateKeySpec>,
    /// Expected `TransactionType`s; empty under the same conditions as
    /// `params`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transaction_types: Vec<String>,
    /// Invocation notes, `None` when no `hook-spec.toml` was found or it
    /// declared none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invoke: Option<InvokeSpec>,
    /// Build facts — always present.
    pub build: BuildInfo,
}

/// Reads `<manifest_dir>/hook-spec.toml` if it exists. Returns `Ok(None)`
/// (not an error) when the file is simply absent — hand-authored specs are
/// opt-in.
pub fn read_input(manifest_dir: &Path) -> Result<Option<HookSpecInput>> {
    let path = manifest_dir.join("hook-spec.toml");
    if !path.is_file() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let input: HookSpecInput =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(input))
}

/// Computes [`BuildInfo`] for a freshly built wasm binary.
#[must_use]
pub fn build_info(
    wasm_file: &str,
    wasm: &[u8],
    api_version: ApiVersion,
    guard_verdict: Option<GuardVerdict>,
) -> BuildInfo {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(wasm);
    let sha256 = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    BuildInfo {
        wasm_file: wasm_file.to_string(),
        size_bytes: wasm.len() as u64,
        sha256,
        hook_api_version: if api_version == ApiVersion::V1 { 1 } else { 0 },
        hook_cost: guard_verdict.map(|v| v.hook_cost),
        cbak_cost: guard_verdict.map(|v| v.cbak_cost),
        sdk_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Merges an optional hand-authored [`HookSpecInput`] with [`BuildInfo`]
/// into the final [`HookSpec`] document. When `input` is `None`, the result
/// carries only `build` (plus the schema version) — the minimal spec that
/// is always written.
#[must_use]
pub fn merge(input: Option<HookSpecInput>, build: BuildInfo) -> HookSpec {
    match input {
        Some(input) => HookSpec {
            spec_version: SPEC_VERSION,
            hook: Some(input.hook),
            params: input.params,
            state: input.state,
            transaction_types: input.transaction_types,
            invoke: input.invoke,
            build,
        },
        None => HookSpec {
            spec_version: SPEC_VERSION,
            hook: None,
            params: Vec::new(),
            state: Vec::new(),
            transaction_types: Vec::new(),
            invoke: None,
            build,
        },
    }
}

/// Writes `spec` as pretty-printed JSON to `<out_dir>/<crate_name>.spec.json`,
/// returning the path written.
pub fn write(spec: &HookSpec, out_dir: &Path, crate_name: &str) -> Result<std::path::PathBuf> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("creating output directory {}", out_dir.display()))?;
    let path = out_dir.join(format!("{crate_name}.spec.json"));
    let json = serde_json::to_string_pretty(spec).context("serializing spec.json")?;
    std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;

    fn sample_build_info() -> BuildInfo {
        BuildInfo {
            wasm_file: "firewall.wasm".to_string(),
            size_bytes: 1234,
            sha256: "0".repeat(64),
            hook_api_version: 0,
            hook_cost: Some(714),
            cbak_cost: Some(0),
            sdk_version: "0.1.0".to_string(),
        }
    }

    /// Regression test for the exact mistake this schema's
    /// `deny_unknown_fields` is meant to catch: a root-level field
    /// (`transaction_types`) placed *after* `[hook]` lands on the `[hook]`
    /// table instead of the document root under TOML's table-context
    /// rules. Without `deny_unknown_fields` this parses successfully and
    /// silently drops the field; with it, `hook-spec.toml` authors get a
    /// parse error pointing at the mistake instead of a missing field they
    /// might not notice.
    #[test]
    fn misplaced_root_field_is_rejected_not_silently_dropped() {
        let toml_src = r#"
[hook]
name = "firewall"
description = "d"

transaction_types = ["Payment"]
"#;
        let result: Result<HookSpecInput, _> = toml::from_str(toml_src);
        assert!(
            result.is_err(),
            "expected a misplaced `transaction_types` (nested under [hook]) to be rejected"
        );
    }

    #[test]
    fn minimal_spec_without_input_is_build_only() {
        let spec = merge(None, sample_build_info());
        assert_eq!(spec.spec_version, SPEC_VERSION);
        assert!(spec.hook.is_none());
        assert!(spec.params.is_empty());
        assert!(spec.state.is_empty());
        assert!(spec.transaction_types.is_empty());
        assert!(spec.invoke.is_none());
        assert_eq!(spec.build, sample_build_info());
    }

    /// Snapshot test: a full `hook-spec.toml` parses, merges, and
    /// serializes to exactly the checked-in fixture JSON. Any change to the
    /// schema or its JSON encoding must be a deliberate, reviewed edit to
    /// the fixture below.
    #[test]
    fn full_spec_matches_snapshot() {
        // NOTE on field order: TOML's bare `key = value` lines belong to
        // whichever table header came before them, so `transaction_types`
        // (a root-level field) must appear before the first `[hook]`/
        // `[invoke]` table header — otherwise it silently becomes an
        // ignored extra field on that table instead of on the document
        // root. `hook-spec.toml`'s docs (`docs/SPEC-SIDECAR.md`) call this
        // out explicitly.
        let toml_src = r#"
transaction_types = ["Payment"]

[hook]
name = "firewall"
description = "Rolls the transaction back if its sender matches a blacklisted account."

[invoke]
hook_on = "Payment"
notes = "Installed with HookOn restricted to Payment; no cbak."

[[params]]
name = "BL"
bytes = 20
description = "20-byte blocked AccountId."

[[state]]
key_name = "counter"
key_derivation = "ascii_padded"
key = "counter"
value_type = "u64"
value_bytes = 8
description = "Monotonic invocation counter."
"#;
        let input: HookSpecInput = toml::from_str(toml_src).expect("valid hook-spec.toml");
        let spec = merge(Some(input), sample_build_info());
        let json = serde_json::to_string_pretty(&spec).expect("serializable");

        let expected = include_str!("../tests/fixtures/firewall.spec.json");
        assert_eq!(json.trim_end(), expected.trim_end());
    }
}
