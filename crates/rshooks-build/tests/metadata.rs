//! Integration tests for build-time Hook metadata extraction and generation.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use rshooks_build::metadata::{
    HookMetadata, METADATA_EXPORT_PREFIX, build_metadata, extract_metadata, hook_hash,
};
use rshooks_build::{ApiVersion, GuardVerdict, Options, ValidationReport};

const COMPACT_METADATA: &str = concat!(
    r#"{"name":"Probe","description":"metadata test","HookOn":["Payment"],"#,
    r#""HookCanEmit":["Invoke"],"HookName":"probe"}"#
);

fn upper_hex(input: &str) -> String {
    input
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect()
}

fn marker_name(json: &str) -> String {
    format!("{METADATA_EXPORT_PREFIX}{}", upper_hex(json))
}

fn raw_with_metadata_json(json: &str) -> Vec<u8> {
    let marker = marker_name(json);
    wat::parse_str(format!(
        r#"
        (module
          (func $hook (param i32) (result i64) i64.const 0)
          (export "hook" (func $hook))
          (export "{marker}" (func $hook)))
        "#
    ))
    .expect("valid metadata carrier fixture")
}

fn source_metadata(hook_can_emit: Option<Vec<String>>) -> HookMetadata {
    HookMetadata {
        name: "Probe".to_string(),
        description: Some("metadata test".to_string()),
        hook_on: Some(vec!["Payment".to_string()]),
        incoming_hook_on: None,
        outgoing_hook_on: None,
        hook_can_emit,
        hook_name: Some("probe".to_string()),
    }
}

fn no_emit_wasm() -> Vec<u8> {
    wat::parse_str(
        r#"
        (module
          (func $hook (param i32) (result i64)
            i64.const 0)
          (export "hook" (func $hook)))
        "#,
    )
    .expect("valid no-emit fixture")
}

fn emit_wasm() -> Vec<u8> {
    wat::parse_str(
        r#"
        (module
          (import "env" "emit" (func $emit (param i32 i32 i32 i32) (result i64)))
          (func $hook (param i32) (result i64)
            i32.const 0
            i32.const 0
            i32.const 0
            i32.const 0
            call $emit
            drop
            i64.const 0)
          (export "hook" (func $hook)))
        "#,
    )
    .expect("valid emit fixture")
}

#[test]
fn extracts_upper_hex_json_from_carrier_export() {
    let marker = marker_name(COMPACT_METADATA);
    let raw = wat::parse_str(format!(
        r#"
        (module
          (func $hook (param i32) (result i64) i64.const 0)
          (func $metadata (param i32) (result i64) i64.const 0)
          (export "hook" (func $hook))
          (export "{marker}" (func $metadata)))
        "#
    ))
    .expect("valid carrier fixture");

    let metadata = extract_metadata(&raw)
        .expect("extract succeeds")
        .expect("metadata is present");
    assert_eq!(metadata, source_metadata(Some(vec!["Invoke".to_string()])));
}

#[test]
fn absent_carrier_is_backward_compatible() {
    assert_eq!(
        extract_metadata(&no_emit_wasm()).expect("scan succeeds"),
        None
    );
}

#[test]
fn malformed_or_multiple_carriers_are_errors() {
    let lowercase = wat::parse_str(format!(
        r#"
        (module
          (func $hook (param i32) (result i64) i64.const 0)
          (export "hook" (func $hook))
          (export "{METADATA_EXPORT_PREFIX}aa" (func $hook)))
        "#
    ))
    .expect("valid lowercase fixture");
    assert!(extract_metadata(&lowercase).is_err());

    let marker = marker_name(COMPACT_METADATA);
    let multiple = wat::parse_str(format!(
        r#"
        (module
          (func $hook (param i32) (result i64) i64.const 0)
          (export "hook" (func $hook))
          (export "{marker}" (func $hook))
          (export "{marker}00" (func $hook)))
        "#
    ))
    .expect("valid multiple fixture");
    let err = extract_metadata(&multiple).expect_err("multiple carriers must fail");
    assert!(err.to_string().contains("multiple"));
}

#[test]
fn rejects_equal_direction_sets_regardless_of_order() {
    let raw = raw_with_metadata_json(
        r#"{"name":"Probe","IncomingHookOn":["Payment","Invoke"],"OutgoingHookOn":["Invoke","Payment"]}"#,
    );
    let err = extract_metadata(&raw).expect_err("equal direction sets must fail");
    assert!(format!("{err:#}").contains("same transaction-type set"));
    assert!(format!("{err:#}").contains("use `HookOn` instead"));
}

#[test]
fn absent_trigger_selection_serializes_as_null_raw_hook_on() {
    let metadata = extract_metadata(&raw_with_metadata_json(r#"{"name":"Probe"}"#))
        .expect("extract succeeds")
        .expect("metadata is present");
    assert_eq!(metadata.hook_on, None);
    assert_eq!(metadata.incoming_hook_on, None);
    assert_eq!(metadata.outgoing_hook_on, None);

    let built = build_metadata(
        metadata,
        &no_emit_wasm(),
        &ValidationReport::default(),
        None,
    )
    .expect("metadata generation succeeds");
    let value = serde_json::to_value(built.document).expect("serialize metadata document");
    assert!(value["HookOn"].is_null());
    assert!(value.get("HookOnIncoming").is_none());
    assert!(value.get("HookOnOutgoing").is_none());
    assert!(value["human"].get("HookOn").is_none());
}

#[test]
fn rejects_unknown_transaction_types_in_every_array() {
    let cases = [
        r#"{"name":"Probe","HookOn":["NotATransaction"]}"#,
        r#"{"name":"Probe","IncomingHookOn":["NotATransaction"],"OutgoingHookOn":["Payment"]}"#,
        r#"{"name":"Probe","IncomingHookOn":["Payment"],"OutgoingHookOn":["NotATransaction"]}"#,
        r#"{"name":"Probe","HookOn":["Payment"],"HookCanEmit":["NotATransaction"]}"#,
    ];

    for json in cases {
        let err = extract_metadata(&raw_with_metadata_json(json))
            .expect_err("unknown transaction type must fail");
        assert!(format!("{err:#}").contains("unknown transaction type"));
    }
}

#[test]
fn carrier_has_no_effect_on_cleaned_wasm() {
    let marker = marker_name(COMPACT_METADATA);
    let baseline = no_emit_wasm();
    let carried = wat::parse_str(format!(
        r#"
        (module
          (type $entry (func (param i32) (result i64)))
          (func $hook (type $entry) i64.const 0)
          (func $metadata (type $entry) i64.const 1)
          (export "hook" (func $hook))
          (export "{marker}" (func $metadata)))
        "#
    ))
    .expect("valid carrier fixture");
    let opts = Options {
        api_version: ApiVersion::V1,
        ..Options::default()
    };

    let (baseline, _) =
        rshooks_build::run_pipeline(&baseline, &opts).expect("baseline pipeline succeeds");
    let (carried, _) =
        rshooks_build::run_pipeline(&carried, &opts).expect("carrier pipeline succeeds");
    assert_eq!(baseline, carried);
}

#[test]
fn hook_hash_matches_sha512_half_known_vector() {
    assert_eq!(
        hook_hash(b"abc"),
        "DDAF35A193617ABACC417349AE20413112E6FA4E89A97EA20A9EEEE64B55D39A"
    );
}

#[test]
fn hook_can_emit_warning_matrix_uses_final_reachable_import() {
    let report = ValidationReport::default();
    let cases = [
        (Some(vec!["Payment".to_string()]), false, 1usize),
        (Some(vec!["Payment".to_string()]), true, 0usize),
        (None, false, 0usize),
        (None, true, 1usize),
    ];

    for (hook_can_emit, uses_emit, expected_warnings) in cases {
        let wasm = if uses_emit {
            emit_wasm()
        } else {
            no_emit_wasm()
        };
        let built = build_metadata(source_metadata(hook_can_emit), &wasm, &report, None)
            .expect("metadata generation succeeds");
        assert_eq!(built.warnings.len(), expected_warnings);
    }
}

#[test]
fn serializes_requested_names_and_nullable_wce() {
    let v0_report = ValidationReport {
        guard_verdict: Some(GuardVerdict {
            hook_cost: 123,
            cbak_cost: 45,
        }),
        ..ValidationReport::default()
    };
    let v0 = build_metadata(source_metadata(None), &no_emit_wasm(), &v0_report, None)
        .expect("v0 metadata generation succeeds");
    let value = serde_json::to_value(v0.document).expect("serialize v0 document");
    assert_eq!(value["name"], "Probe");
    assert_eq!(
        value["HookOn"],
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFBFFFFE"
    );
    assert!(value["HookCanEmit"].is_null());
    assert_eq!(value["HookName"], "70726F6265");
    assert_eq!(value["human"]["HookOn"][0], "Payment");
    assert!(value.get("HookOnIncoming").is_none());
    assert!(value.get("HookOnOutgoing").is_none());
    assert!(value["human"].get("HookOnIncoming").is_none());
    assert!(value["human"].get("HookOnOutgoing").is_none());
    assert_eq!(value["human"]["HookName"], "probe");
    assert!(value["human"]["HookCanEmit"].is_null());
    assert!(value["human"].get("HookHash").is_none());
    assert_eq!(value["WCE"]["hook"], 123);
    assert_eq!(value["WCE"]["cbak"], 45);
    assert!(
        value["HookHash"]
            .as_str()
            .is_some_and(|hash| hash.len() == 64)
    );
    assert_eq!(value["builder"]["name"], "rshooks-build");
    assert_eq!(value["builder"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(value["builder"]["rustc"].is_null());

    let v1 = build_metadata(
        source_metadata(None),
        &no_emit_wasm(),
        &ValidationReport::default(),
        None,
    )
    .expect("v1 metadata generation succeeds");
    let value = serde_json::to_value(v1.document).expect("serialize v1 document");
    assert!(value["WCE"]["hook"].is_null());
    assert!(value["WCE"]["cbak"].is_null());
}

#[test]
fn builder_provenance_carries_detected_rustc_and_sits_between_wce_and_human() {
    let built = build_metadata(
        source_metadata(None),
        &no_emit_wasm(),
        &ValidationReport::default(),
        Some("rustc 1.89.0 (29483883e 2025-08-04)".to_string()),
    )
    .expect("metadata generation succeeds");
    let value = serde_json::to_value(&built.document).expect("serialize metadata document");

    assert_eq!(value["builder"]["name"], "rshooks-build");
    assert_eq!(value["builder"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        value["builder"]["rustc"],
        "rustc 1.89.0 (29483883e 2025-08-04)"
    );

    let bytes = serde_json::to_vec(&built.document).expect("serialize to bytes");
    let text = String::from_utf8(bytes).expect("valid utf-8");
    let wce_pos = text.find("\"WCE\"").expect("WCE key present");
    let builder_pos = text.find("\"builder\"").expect("builder key present");
    let human_pos = text.find("\"human\"").expect("human key present");
    assert!(
        wce_pos < builder_pos && builder_pos < human_pos,
        "expected key order WCE, builder, human; got: {text}"
    );

    let builder_name_pos = text
        .find("\"name\":\"rshooks-build\"")
        .expect("builder.name key present");
    let builder_version_pos = text
        .find("\"version\"")
        .expect("builder.version key present");
    let builder_rustc_pos = text.find("\"rustc\"").expect("builder.rustc key present");
    assert!(
        builder_pos < builder_name_pos
            && builder_name_pos < builder_version_pos
            && builder_version_pos < builder_rustc_pos,
        "expected builder field order name, version, rustc; got: {text}"
    );
}

#[test]
fn serializes_directional_trigger_masks_and_human_values() {
    let metadata = HookMetadata {
        name: "Directional".to_string(),
        description: None,
        hook_on: None,
        incoming_hook_on: Some(vec!["Payment".to_string(), "Invoke".to_string()]),
        outgoing_hook_on: Some(vec!["OfferCreate".to_string()]),
        hook_can_emit: Some(vec!["Payment".to_string()]),
        hook_name: Some("支払".to_string()),
    };
    let built = build_metadata(
        metadata,
        &no_emit_wasm(),
        &ValidationReport::default(),
        None,
    )
    .expect("metadata generation succeeds");
    let value = serde_json::to_value(built.document).expect("serialize metadata document");

    assert!(value.get("HookOn").is_none());
    assert_eq!(
        value["HookOnIncoming"],
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF7FFFFFFFFFFFFFFFFFFBFFFFE"
    );
    assert_eq!(
        value["HookOnOutgoing"],
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFBFFF7F"
    );
    assert_eq!(
        value["HookCanEmit"],
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFBFFFFE"
    );
    assert_eq!(value["HookName"], "E694AFE68995");
    assert_eq!(value["human"]["HookOnIncoming"][1], "Invoke");
    assert_eq!(value["human"]["HookOnOutgoing"][0], "OfferCreate");
    assert!(value["human"].get("HookOn").is_none());
    assert_eq!(value["human"]["HookCanEmit"][0], "Payment");
    assert_eq!(value["human"]["HookName"], "支払");
}

#[test]
fn distinguishes_absent_and_explicitly_empty_hook_can_emit() {
    let built = build_metadata(
        source_metadata(Some(Vec::new())),
        &no_emit_wasm(),
        &ValidationReport::default(),
        None,
    )
    .expect("metadata generation succeeds");
    let value = serde_json::to_value(built.document).expect("serialize metadata document");

    assert_eq!(
        value["HookCanEmit"],
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFBFFFFF"
    );
    assert!(
        value["human"]["HookCanEmit"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
}

#[test]
fn warns_when_hook_name_character_length_is_valid_but_protocol_bytes_are_not() {
    let report = ValidationReport::default();

    let mut too_short = source_metadata(None);
    too_short.hook_name = Some("ab".to_string());
    let built = build_metadata(too_short, &no_emit_wasm(), &report, None)
        .expect("short HookName metadata generation succeeds");
    assert_eq!(built.warnings.len(), 1);
    assert!(built.warnings[0].contains("2 UTF-8 bytes"));
    assert!(built.warnings[0].contains("requires 4 to 16 bytes"));

    let mut too_long = source_metadata(None);
    too_long.hook_name = Some("日本語日本語".to_string());
    let built = build_metadata(too_long, &no_emit_wasm(), &report, None)
        .expect("long HookName metadata generation succeeds");
    assert_eq!(built.warnings.len(), 1);
    assert!(built.warnings[0].contains("18 UTF-8 bytes"));

    let mut protocol_compatible = source_metadata(None);
    protocol_compatible.hook_name = Some("éé".to_string());
    let built = build_metadata(protocol_compatible, &no_emit_wasm(), &report, None)
        .expect("protocol-compatible HookName metadata generation succeeds");
    assert!(built.warnings.is_empty());
}
