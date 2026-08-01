//! Build-time Hook metadata extraction and sidecar generation.
//!
//! Hook crates carry source metadata in the name of a deliberately dead wasm
//! export. The cleaner removes that carrier from the deployable module; this
//! module reads it from cargo's raw artifact first, then combines it with facts
//! derived from the final cleaned bytes.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};

use crate::ValidationReport;

/// Prefix used by `metadata!` carrier exports in raw Hook wasm artifacts.
pub const METADATA_EXPORT_PREFIX: &str = "__rhooks_metadata_v1_";

/// Canonical Xahau JSON spellings emitted by the current `metadata!` macro
/// for every known `TxType` variant (excluding the data-carrying `Unknown`).
const TRANSACTION_TYPES: &[&str] = &[
    "Payment",
    "EscrowCreate",
    "EscrowFinish",
    "AccountSet",
    "EscrowCancel",
    "SetRegularKey",
    "OfferCreate",
    "OfferCancel",
    "TicketCreate",
    "SignerListSet",
    "PaymentChannelCreate",
    "PaymentChannelFund",
    "PaymentChannelClaim",
    "CheckCreate",
    "CheckCash",
    "CheckCancel",
    "DepositPreauth",
    "TrustSet",
    "AccountDelete",
    "SetHook",
    "NFTokenMint",
    "NFTokenBurn",
    "NFTokenCreateOffer",
    "NFTokenCancelOffer",
    "NFTokenAcceptOffer",
    "Clawback",
    "AMMClawback",
    "AMMCreate",
    "AMMDeposit",
    "AMMWithdraw",
    "AMMVote",
    "AMMBid",
    "AMMDelete",
    "URITokenMint",
    "URITokenBurn",
    "URITokenBuy",
    "URITokenCreateSellOffer",
    "URITokenCancelSellOffer",
    "XChainCreateClaimID",
    "XChainCommit",
    "XChainClaim",
    "XChainAccountCreateCommit",
    "XChainAddClaimAttestation",
    "XChainAddAccountCreateAttestation",
    "XChainModifyBridge",
    "XChainCreateBridge",
    "DIDSet",
    "DIDDelete",
    "OracleSet",
    "OracleDelete",
    "LedgerStateFix",
    "MPTokenIssuanceCreate",
    "MPTokenIssuanceDestroy",
    "MPTokenIssuanceSet",
    "MPTokenAuthorize",
    "CredentialCreate",
    "CredentialAccept",
    "CredentialDelete",
    "NFTokenModify",
    "PermissionedDomainSet",
    "PermissionedDomainDelete",
    "Cron",
    "CronSet",
    "SetRemarks",
    "Remit",
    "GenesisMint",
    "Import",
    "ClaimReward",
    "Invoke",
    "EnableAmendment",
    "SetFee",
    "UNLModify",
    "EmitFailure",
    "UNLReport",
];

/// Source-authored metadata recovered from a `metadata!` carrier export.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookMetadata {
    /// Human-readable Hook name.
    pub name: String,
    /// Optional longer description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Transaction types which trigger the Hook in both directions.
    #[serde(default, rename = "HookOn", skip_serializing_if = "Option::is_none")]
    pub hook_on: Option<Vec<String>>,
    /// Transaction types which trigger the Hook for incoming transactions.
    #[serde(
        default,
        rename = "IncomingHookOn",
        skip_serializing_if = "Option::is_none"
    )]
    pub incoming_hook_on: Option<Vec<String>>,
    /// Transaction types which trigger the Hook for outgoing transactions.
    #[serde(
        default,
        rename = "OutgoingHookOn",
        skip_serializing_if = "Option::is_none"
    )]
    pub outgoing_hook_on: Option<Vec<String>>,
    /// Optional transaction types the Hook declares it may emit.
    #[serde(
        default,
        rename = "HookCanEmit",
        skip_serializing_if = "Option::is_none"
    )]
    pub hook_can_emit: Option<Vec<String>>,
    /// Optional UTF-8 name placed in SetHook's `HookName` field.
    #[serde(default, rename = "HookName", skip_serializing_if = "Option::is_none")]
    pub hook_name: Option<String>,
}

impl HookMetadata {
    fn validate(&self) -> Result<()> {
        if self.name.is_empty() {
            bail!("metadata field `name` must not be empty");
        }

        match (
            self.hook_on.is_some(),
            self.incoming_hook_on.is_some(),
            self.outgoing_hook_on.is_some(),
        ) {
            (true, false, false) | (false, true, true) => {}
            _ => bail!(
                "metadata must contain either `HookOn` or both `IncomingHookOn` and \
                 `OutgoingHookOn`, but never both forms"
            ),
        }

        validate_transaction_types("HookOn", self.hook_on.as_deref())?;
        validate_transaction_types("IncomingHookOn", self.incoming_hook_on.as_deref())?;
        validate_transaction_types("OutgoingHookOn", self.outgoing_hook_on.as_deref())?;
        validate_transaction_types("HookCanEmit", self.hook_can_emit.as_deref())?;

        if let (Some(incoming), Some(outgoing)) = (&self.incoming_hook_on, &self.outgoing_hook_on) {
            let incoming: BTreeSet<&str> = incoming.iter().map(String::as_str).collect();
            let outgoing: BTreeSet<&str> = outgoing.iter().map(String::as_str).collect();
            if incoming == outgoing {
                bail!(
                    "metadata fields `IncomingHookOn` and `OutgoingHookOn` must not contain the \
                     same transaction-type set"
                );
            }
        }

        if let Some(name) = &self.hook_name {
            let char_count = name.chars().count();
            if !(2..=8).contains(&char_count) {
                bail!("metadata field `HookName` must contain 2 to 8 UTF-8 characters");
            }
        }

        Ok(())
    }
}

fn validate_transaction_types(field: &str, values: Option<&[String]>) -> Result<()> {
    let Some(values) = values else {
        return Ok(());
    };
    let mut seen = BTreeSet::new();
    for value in values {
        if value.is_empty() {
            bail!("metadata field `{field}` contains an empty transaction type");
        }
        if !TRANSACTION_TYPES.contains(&value.as_str()) {
            bail!("metadata field `{field}` contains unknown transaction type `{value}`");
        }
        if !seen.insert(value) {
            bail!("metadata field `{field}` contains duplicate transaction type `{value}`");
        }
    }
    Ok(())
}

/// Worst-case instruction counts for the Hook entry points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WorstCaseExecution {
    /// Static WCE for `hook`, or `null` for gas-type Hooks.
    pub hook: Option<u64>,
    /// Static WCE for `cbak`, or `null` for gas-type Hooks.
    pub cbak: Option<u64>,
}

/// Complete JSON sidecar document written next to a built Hook wasm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MetadataDocument {
    /// Metadata authored in the Hook crate.
    #[serde(flatten)]
    pub metadata: HookMetadata,
    /// SHA512-Half of the exact final cleaned wasm bytes.
    #[serde(rename = "HookHash")]
    pub hook_hash: String,
    /// Worst-case instruction counts, when statically available.
    #[serde(rename = "WCE")]
    pub wce: WorstCaseExecution,
}

/// Generated metadata document and non-fatal source/binary consistency warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataBuild {
    /// The serializable sidecar document.
    pub document: MetadataDocument,
    /// Warnings comparing `HookCanEmit` to final reachable `env::emit` use.
    pub warnings: Vec<String>,
}

/// Extracts a single metadata carrier from a raw cargo-produced wasm artifact.
///
/// Returns `Ok(None)` for Hooks that do not use `metadata!`. Multiple carriers,
/// non-function carriers, malformed uppercase hex, invalid JSON, and invalid
/// metadata field combinations are hard errors.
pub fn extract_metadata(wasm: &[u8]) -> Result<Option<HookMetadata>> {
    let mut metadata = None;

    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        let payload = payload.context("parsing raw wasm while looking for Hook metadata")?;
        let wasmparser::Payload::ExportSection(reader) = payload else {
            continue;
        };
        for export in reader {
            let export = export.context("reading raw wasm export while looking for metadata")?;
            let Some(encoded) = export.name.strip_prefix(METADATA_EXPORT_PREFIX) else {
                continue;
            };
            if metadata.is_some() {
                bail!("raw wasm contains multiple Hook metadata carrier exports");
            }
            if export.kind != wasmparser::ExternalKind::Func {
                bail!("Hook metadata carrier export must refer to a function");
            }

            let json = decode_upper_hex(encoded).context("decoding Hook metadata carrier")?;
            let parsed: HookMetadata =
                serde_json::from_slice(&json).context("parsing JSON from Hook metadata carrier")?;
            parsed.validate().context("validating Hook metadata")?;
            metadata = Some(parsed);
        }
    }

    Ok(metadata)
}

/// Creates the final sidecar document from source metadata and final wasm facts.
pub fn build_metadata(
    metadata: HookMetadata,
    final_wasm: &[u8],
    report: &ValidationReport,
) -> Result<MetadataBuild> {
    let uses_emit = uses_reachable_emit(final_wasm)?;
    let mut warnings = Vec::new();
    match (metadata.hook_can_emit.is_some(), uses_emit) {
        (true, false) => warnings.push(
            "metadata specifies `HookCanEmit`, but the final wasm does not use the `emit` API"
                .to_string(),
        ),
        (false, true) => warnings.push(
            "the final wasm uses the `emit` API, but metadata does not specify `HookCanEmit`"
                .to_string(),
        ),
        _ => {}
    }

    if let Some(name) = &metadata.hook_name {
        let byte_count = name.len();
        if !(4..=16).contains(&byte_count) {
            warnings.push(format!(
                "metadata `HookName` is {byte_count} UTF-8 bytes, but the current Xahau protocol \
                 requires 4 to 16 bytes"
            ));
        }
    }

    let wce = report.guard_verdict.map_or(
        WorstCaseExecution {
            hook: None,
            cbak: None,
        },
        |verdict| WorstCaseExecution {
            hook: Some(verdict.hook_cost),
            cbak: Some(verdict.cbak_cost),
        },
    );

    Ok(MetadataBuild {
        document: MetadataDocument {
            metadata,
            hook_hash: hook_hash(final_wasm),
            wce,
        },
        warnings,
    })
}

/// Computes Xahau's HookHash: the uppercase first 32 bytes of SHA-512.
#[must_use]
pub fn hook_hash(wasm: &[u8]) -> String {
    let digest = Sha512::digest(wasm);
    let mut out = String::with_capacity(64);
    for byte in digest.iter().take(32) {
        // Writing to a String cannot fail.
        let _ = write!(out, "{byte:02X}");
    }
    out
}

fn uses_reachable_emit(wasm: &[u8]) -> Result<bool> {
    let module = crate::ir::parse(wasm).context("parsing final wasm for `emit` usage")?;
    Ok(module.find_func_import("env", "emit").is_some())
}

fn decode_upper_hex(encoded: &str) -> Result<Vec<u8>> {
    if encoded.is_empty() {
        bail!("metadata carrier payload is empty");
    }
    if encoded.len() % 2 != 0 {
        bail!("metadata carrier payload has an odd number of hex digits");
    }

    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let high = pair
            .first()
            .copied()
            .context("metadata carrier hex pair is missing its first digit")?;
        let low = pair
            .get(1)
            .copied()
            .context("metadata carrier hex pair is missing its second digit")?;
        decoded.push((upper_hex_value(high)? << 4) | upper_hex_value(low)?);
    }
    Ok(decoded)
}

fn upper_hex_value(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => bail!("metadata carrier contains non-uppercase-hex byte 0x{byte:02X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_transaction_type_whitelist_has_all_current_variants() {
        assert_eq!(TRANSACTION_TYPES.len(), 74);
        assert_eq!(
            TRANSACTION_TYPES
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len(),
            TRANSACTION_TYPES.len(),
            "canonical TransactionType names must be unique"
        );
    }
}
