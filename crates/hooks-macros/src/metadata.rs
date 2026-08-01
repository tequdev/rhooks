//! Parser and carrier code generation for `metadata!`.
//!
//! The metadata is serialized by this host-side proc macro, hex-encoded into
//! a distinctive wasm export name, and extracted by `hooks-build` before its
//! cleaner removes the export. No metadata bytes are placed in linear memory.

use proc_macro::{Delimiter, Ident, Span, TokenStream, TokenTree};
use std::collections::BTreeSet;

use crate::{err, sha256};

const EXPORT_PREFIX: &str = "__rhooks_metadata_v1_";

#[derive(Clone)]
struct StringValue {
    value: String,
    span: Span,
}

#[derive(Clone)]
struct Variant {
    name: String,
}

#[derive(Default)]
struct Metadata {
    name: Option<StringValue>,
    description: Option<StringValue>,
    hook_on: Option<Vec<Variant>>,
    incoming_hook_on: Option<Vec<Variant>>,
    outgoing_hook_on: Option<Vec<Variant>>,
    hook_can_emit: Option<Vec<Variant>>,
    hook_name: Option<StringValue>,
}

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let metadata = match parse(input) {
        Ok(metadata) => metadata,
        Err(error) => return error,
    };

    let payload = match encode_json(&metadata) {
        Ok(payload) => payload,
        Err(message) => return err(Span::call_site(), &message),
    };
    let payload_hex = hex_upper(&payload);
    let digest = sha256::sha256(&payload);
    let carrier_ident = format!("__rhooks_metadata_{}", hex_lower(&digest));

    let mut generated = String::new();
    for variant in variants(&metadata) {
        generated.push_str(&format!(
            "const _: ::hooks_lib::tx_type::TxType = \
             ::hooks_lib::tx_type::TxType::{};",
            variant.name
        ));
    }
    generated.push_str(&format!(
        "#[cfg(target_arch = \"wasm32\")]\n\
         #[doc(hidden)]\n\
         #[unsafe(export_name = \"{EXPORT_PREFIX}{payload_hex}\")]\n\
         pub extern \"C\" fn {carrier_ident}(_reserved: u32) -> i64 {{ 0 }}"
    ));

    generated.parse::<TokenStream>().unwrap_or_else(|_| {
        err(
            Span::call_site(),
            "hooks-macros: internal metadata! expansion failed",
        )
    })
}

fn parse(input: TokenStream) -> Result<Metadata, TokenStream> {
    let tokens: Vec<TokenTree> = input.into_iter().collect();
    let mut cursor = 0usize;
    let mut metadata = Metadata::default();

    while cursor < tokens.len() {
        let field = match tokens.get(cursor) {
            Some(TokenTree::Ident(ident)) => ident.clone(),
            Some(other) => {
                return Err(err(other.span(), "metadata!: expected a field name"));
            }
            None => break,
        };
        cursor = cursor.wrapping_add(1);

        match tokens.get(cursor) {
            Some(TokenTree::Punct(punct)) if punct.as_char() == ':' => {}
            Some(other) => {
                return Err(err(
                    other.span(),
                    "metadata!: expected `:` after the field name",
                ));
            }
            None => {
                return Err(err(
                    field.span(),
                    "metadata!: expected `:` after the field name",
                ));
            }
        }
        cursor = cursor.wrapping_add(1);

        let field_name = field.to_string();
        match field_name.as_str() {
            "name" => {
                reject_duplicate(&metadata.name, &field, "name")?;
                metadata.name = Some(parse_string(&tokens, &mut cursor, &field)?);
            }
            "description" => {
                reject_duplicate(&metadata.description, &field, "description")?;
                metadata.description = Some(parse_string(&tokens, &mut cursor, &field)?);
            }
            "HookOn" => {
                reject_duplicate(&metadata.hook_on, &field, "HookOn")?;
                metadata.hook_on = Some(parse_variants(&tokens, &mut cursor, &field)?);
            }
            "IncomingHookOn" => {
                reject_duplicate(&metadata.incoming_hook_on, &field, "IncomingHookOn")?;
                metadata.incoming_hook_on = Some(parse_variants(&tokens, &mut cursor, &field)?);
            }
            "OutgoingHookOn" => {
                reject_duplicate(&metadata.outgoing_hook_on, &field, "OutgoingHookOn")?;
                metadata.outgoing_hook_on = Some(parse_variants(&tokens, &mut cursor, &field)?);
            }
            "HookCanEmit" => {
                reject_duplicate(&metadata.hook_can_emit, &field, "HookCanEmit")?;
                metadata.hook_can_emit = Some(parse_variants(&tokens, &mut cursor, &field)?);
            }
            "HookName" => {
                reject_duplicate(&metadata.hook_name, &field, "HookName")?;
                metadata.hook_name = Some(parse_string(&tokens, &mut cursor, &field)?);
            }
            _ => {
                return Err(err(
                    field.span(),
                    &format!("metadata!: unknown field `{field_name}`"),
                ));
            }
        }

        if cursor < tokens.len() {
            match tokens.get(cursor) {
                Some(TokenTree::Punct(punct)) if punct.as_char() == ',' => {
                    cursor = cursor.wrapping_add(1);
                }
                Some(other) => {
                    return Err(err(other.span(), "metadata!: expected `,` between fields"));
                }
                None => {}
            }
        }
    }

    validate(metadata)
}

fn reject_duplicate<T>(
    value: &Option<T>,
    field: &Ident,
    field_name: &str,
) -> Result<(), TokenStream> {
    if value.is_some() {
        Err(err(
            field.span(),
            &format!("metadata!: duplicate field `{field_name}`"),
        ))
    } else {
        Ok(())
    }
}

fn parse_string(
    tokens: &[TokenTree],
    cursor: &mut usize,
    field: &Ident,
) -> Result<StringValue, TokenStream> {
    let literal = match tokens.get(*cursor) {
        Some(TokenTree::Literal(literal)) => literal.clone(),
        Some(other) => {
            return Err(err(
                other.span(),
                &format!("metadata!: `{field}` must be a string literal"),
            ));
        }
        None => {
            return Err(err(
                field.span(),
                &format!("metadata!: `{field}` must be a string literal"),
            ));
        }
    };
    *cursor = cursor.wrapping_add(1);

    let span = literal.span();
    let mut literal_stream = TokenStream::new();
    literal_stream.extend([TokenTree::Literal(literal)]);
    let value = syn::parse::<syn::LitStr>(literal_stream)
        .map_err(|_| {
            err(
                span,
                &format!("metadata!: `{field}` must be a string literal"),
            )
        })?
        .value();

    Ok(StringValue { value, span })
}

fn parse_variants(
    tokens: &[TokenTree],
    cursor: &mut usize,
    field: &Ident,
) -> Result<Vec<Variant>, TokenStream> {
    let group = match tokens.get(*cursor) {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Bracket => group.clone(),
        Some(other) => {
            return Err(err(
                other.span(),
                &format!("metadata!: `{field}` must be an array of bare `TxType` variant names"),
            ));
        }
        None => {
            return Err(err(
                field.span(),
                &format!("metadata!: `{field}` must be an array of bare `TxType` variant names"),
            ));
        }
    };
    *cursor = cursor.wrapping_add(1);

    let inner: Vec<TokenTree> = group.stream().into_iter().collect();
    let mut variants = Vec::new();
    let mut inner_cursor = 0usize;
    while inner_cursor < inner.len() {
        let ident = match inner.get(inner_cursor) {
            Some(TokenTree::Ident(ident)) => ident,
            Some(other) => {
                return Err(err(
                    other.span(),
                    &format!("metadata!: `{field}` entries must be bare `TxType` variant names"),
                ));
            }
            None => break,
        };
        if variants
            .iter()
            .any(|variant: &Variant| variant.name == ident.to_string())
        {
            return Err(err(
                ident.span(),
                &format!("metadata!: duplicate transaction type `{ident}` in `{field}`"),
            ));
        }
        variants.push(Variant {
            name: ident.to_string(),
        });
        inner_cursor = inner_cursor.wrapping_add(1);

        if inner_cursor < inner.len() {
            match inner.get(inner_cursor) {
                Some(TokenTree::Punct(punct)) if punct.as_char() == ',' => {
                    inner_cursor = inner_cursor.wrapping_add(1);
                }
                Some(other) => {
                    return Err(err(
                        other.span(),
                        &format!("metadata!: expected `,` between `{field}` entries"),
                    ));
                }
                None => {}
            }
        }
    }

    Ok(variants)
}

fn validate(metadata: Metadata) -> Result<Metadata, TokenStream> {
    let Some(name) = &metadata.name else {
        return Err(err(
            Span::call_site(),
            "metadata!: missing required field `name`",
        ));
    };
    if name.value.is_empty() {
        return Err(err(name.span, "metadata!: `name` must not be empty"));
    }

    match (
        metadata.hook_on.is_some(),
        metadata.incoming_hook_on.is_some(),
        metadata.outgoing_hook_on.is_some(),
    ) {
        (true, false, false) | (false, true, true) => {}
        (true, _, _) => {
            return Err(err(
                Span::call_site(),
                "metadata!: use either `HookOn` or the `IncomingHookOn` + \
                 `OutgoingHookOn` pair, not both",
            ));
        }
        (false, false, false) => {
            return Err(err(
                Span::call_site(),
                "metadata!: missing required `HookOn` or `IncomingHookOn` + \
                 `OutgoingHookOn` pair",
            ));
        }
        (false, true, false) | (false, false, true) => {
            return Err(err(
                Span::call_site(),
                "metadata!: `IncomingHookOn` and `OutgoingHookOn` must be specified together",
            ));
        }
    }

    if let Some(hook_name) = &metadata.hook_name {
        let chars = hook_name.value.chars().count();
        if !(2..=8).contains(&chars) {
            return Err(err(
                hook_name.span,
                "metadata!: `HookName` must contain 2 to 8 Unicode scalar values",
            ));
        }
    }

    if let (Some(incoming), Some(outgoing)) =
        (&metadata.incoming_hook_on, &metadata.outgoing_hook_on)
    {
        let incoming: BTreeSet<&str> = incoming
            .iter()
            .map(|variant| variant.name.as_str())
            .collect();
        let outgoing: BTreeSet<&str> = outgoing
            .iter()
            .map(|variant| variant.name.as_str())
            .collect();
        if incoming == outgoing {
            return Err(err(
                Span::call_site(),
                "metadata!: `IncomingHookOn` and `OutgoingHookOn` must not contain the same \
                 transaction-type set",
            ));
        }
    }

    Ok(metadata)
}

fn encode_json(metadata: &Metadata) -> Result<Vec<u8>, String> {
    let mut object = serde_json::Map::new();
    if let Some(name) = &metadata.name {
        object.insert("name".to_string(), name.value.clone().into());
    }
    if let Some(description) = &metadata.description {
        object.insert("description".to_string(), description.value.clone().into());
    }
    insert_variants(&mut object, "HookOn", metadata.hook_on.as_deref());
    insert_variants(
        &mut object,
        "IncomingHookOn",
        metadata.incoming_hook_on.as_deref(),
    );
    insert_variants(
        &mut object,
        "OutgoingHookOn",
        metadata.outgoing_hook_on.as_deref(),
    );
    insert_variants(
        &mut object,
        "HookCanEmit",
        metadata.hook_can_emit.as_deref(),
    );
    if let Some(hook_name) = &metadata.hook_name {
        object.insert("HookName".to_string(), hook_name.value.clone().into());
    }

    serde_json::to_vec(&serde_json::Value::Object(object))
        .map_err(|error| format!("metadata!: failed to serialize JSON: {error}"))
}

fn insert_variants(
    object: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
    variants: Option<&[Variant]>,
) {
    let Some(variants) = variants else {
        return;
    };
    object.insert(
        field.to_string(),
        variants
            .iter()
            .map(|variant| serde_json::Value::String(canonical_name(&variant.name).to_string()))
            .collect::<Vec<_>>()
            .into(),
    );
}

/// Converts the generated Rust `TxType` spelling to Xahau's canonical JSON
/// `TransactionType` spelling. Variants not needing acronym or verb-order
/// repair intentionally pass through unchanged.
fn canonical_name(rust_name: &str) -> &str {
    match rust_name {
        "RegularKeySet" => "SetRegularKey",
        "PaychanCreate" => "PaymentChannelCreate",
        "PaychanFund" => "PaymentChannelFund",
        "PaychanClaim" => "PaymentChannelClaim",
        "HookSet" => "SetHook",
        "NftokenMint" => "NFTokenMint",
        "NftokenBurn" => "NFTokenBurn",
        "NftokenCreateOffer" => "NFTokenCreateOffer",
        "NftokenCancelOffer" => "NFTokenCancelOffer",
        "NftokenAcceptOffer" => "NFTokenAcceptOffer",
        "NftokenModify" => "NFTokenModify",
        "AmmClawback" => "AMMClawback",
        "AmmCreate" => "AMMCreate",
        "AmmDeposit" => "AMMDeposit",
        "AmmWithdraw" => "AMMWithdraw",
        "AmmVote" => "AMMVote",
        "AmmBid" => "AMMBid",
        "AmmDelete" => "AMMDelete",
        "UritokenMint" => "URITokenMint",
        "UritokenBurn" => "URITokenBurn",
        "UritokenBuy" => "URITokenBuy",
        "UritokenCreateSellOffer" => "URITokenCreateSellOffer",
        "UritokenCancelSellOffer" => "URITokenCancelSellOffer",
        "XchainCreateClaimId" => "XChainCreateClaimID",
        "XchainCommit" => "XChainCommit",
        "XchainClaim" => "XChainClaim",
        "XchainAccountCreateCommit" => "XChainAccountCreateCommit",
        "XchainAddClaimAttestation" => "XChainAddClaimAttestation",
        "XchainAddAccountCreateAttestation" => "XChainAddAccountCreateAttestation",
        "XchainModifyBridge" => "XChainModifyBridge",
        "XchainCreateBridge" => "XChainCreateBridge",
        "DidSet" => "DIDSet",
        "DidDelete" => "DIDDelete",
        "MptokenIssuanceCreate" => "MPTokenIssuanceCreate",
        "MptokenIssuanceDestroy" => "MPTokenIssuanceDestroy",
        "MptokenIssuanceSet" => "MPTokenIssuanceSet",
        "MptokenAuthorize" => "MPTokenAuthorize",
        "RemarksSet" => "SetRemarks",
        "Amendment" => "EnableAmendment",
        "Fee" => "SetFee",
        "UnlModify" => "UNLModify",
        "UnlReport" => "UNLReport",
        other => other,
    }
}

fn variants(metadata: &Metadata) -> impl Iterator<Item = &Variant> {
    metadata
        .hook_on
        .iter()
        .chain(&metadata.incoming_hook_on)
        .chain(&metadata.outgoing_hook_on)
        .chain(&metadata.hook_can_emit)
        .flatten()
}

fn hex_upper(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::canonical_name;

    #[test]
    fn canonical_transaction_type_spellings() {
        let cases = [
            ("RegularKeySet", "SetRegularKey"),
            ("PaychanCreate", "PaymentChannelCreate"),
            ("PaychanFund", "PaymentChannelFund"),
            ("PaychanClaim", "PaymentChannelClaim"),
            ("HookSet", "SetHook"),
            ("NftokenMint", "NFTokenMint"),
            ("AmmCreate", "AMMCreate"),
            ("UritokenMint", "URITokenMint"),
            ("XchainCreateClaimId", "XChainCreateClaimID"),
            ("DidSet", "DIDSet"),
            ("MptokenAuthorize", "MPTokenAuthorize"),
            ("RemarksSet", "SetRemarks"),
            ("Amendment", "EnableAmendment"),
            ("Fee", "SetFee"),
            ("UnlModify", "UNLModify"),
            ("Payment", "Payment"),
        ];

        for (rust_name, expected) in cases {
            assert_eq!(canonical_name(rust_name), expected);
        }
    }
}
