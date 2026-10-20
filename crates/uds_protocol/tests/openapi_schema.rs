//! Checks the generated `OpenAPI` document against what `serde` does.
//!
//! The `utoipa` half of this crate is hand-written for every type whose serialized form is
//! a single protocol byte, and for the composite requests that deserialize through a repr.
//! These tests check that the document resolves every `$ref`, that each byte schema's
//! `maximum` is the largest byte its deserializer accepts, and that no published
//! description is empty, truncated or names a crate-private type.

// Test code: `unwrap`, `expect` and `panic` are the failure mechanism. An integration test
// is its own crate, so lib.rs cannot relax the workspace lints for it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
#![cfg(all(feature = "utoipa", feature = "serde"))]
// utoipa's `OpenApi` derive expands to `iter().for_each(..)`, so this lint fires on
// generated code that this file cannot change.
#![allow(clippy::needless_for_each)]

use std::collections::BTreeSet;

use utoipa::{OpenApi, PartialSchema};

#[derive(OpenApi)]
#[openapi(components(schemas(
    // The four composites with hand-written schemas that delegate to a repr.
    uds_protocol::CommunicationControlRequest,
    uds_protocol::TesterPresentRequest,
    uds_protocol::TesterPresentResponse,
    uds_protocol::RequestDownloadRequest,
    uds_protocol::RequestUploadRequest,
)))]
struct Api;

fn walk(value: &serde_json::Value, found: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                if key == "$ref"
                    && let Some(name) = val
                        .as_str()
                        .and_then(|s| s.strip_prefix("#/components/schemas/"))
                {
                    found.insert(name.to_owned());
                }
                walk(val, found);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                walk(item, found);
            }
        }
        _ => {}
    }
}

/// Every `$ref` in `doc` that points at `#/components/schemas/...`.
fn referenced_schemas(doc: &serde_json::Value) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    walk(doc, &mut found);
    found
}

#[test]
fn the_document_has_no_dangling_refs() {
    let json = Api::openapi().to_json().expect("serializes");
    let doc: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    let schemas = doc["components"]["schemas"]
        .as_object()
        .expect("components.schemas");

    let referenced = referenced_schemas(&doc);
    let dangling: Vec<&String> = referenced
        .iter()
        .filter(|name| !schemas.contains_key(name.as_str()))
        .collect();

    assert!(
        dangling.is_empty(),
        "these schemas are referenced but never defined, so the document does not resolve: \
         {dangling:?}. A hand-written `ToSchema` must forward `schemas()`, which defaults \
         to a no-op."
    );

    // And the children really are pulled in, so the assertion above does not pass
    // vacuously on a document with no `$ref`s at all.
    assert!(
        schemas.contains_key("SubnetNumber"),
        "expected the composite request to register its child schemas, got {:?}",
        schemas.keys().collect::<Vec<_>>()
    );
}

/// `maximum` on each byte schema is the largest byte `serde` accepts, discovered by
/// deserializing all 256 bytes rather than hard-coded.
macro_rules! assert_schema_bound_matches_serde {
    ($ty:ty, $name:literal) => {{
        let accepted: Vec<u8> = (0u8..=0xFF)
            .filter(|b| serde_json::from_str::<$ty>(&b.to_string()).is_ok())
            .collect();
        let highest_accepted = *accepted
            .last()
            .expect(concat!($name, " accepts no byte at all"));

        let schema = serde_json::to_value(<$ty as PartialSchema>::schema()).expect("serializes");
        let advertised = schema
            .get("maximum")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_else(|| {
                panic!(
                    "{} has no `maximum`, so it advertises the whole i32 range while rejecting \
                     everything above {:#04X}",
                    $name, highest_accepted
                )
            });

        assert_eq!(
            advertised,
            u64::from(highest_accepted),
            "{}: schema says maximum {}, but serde accepts up to {:#04X}",
            $name,
            advertised,
            highest_accepted
        );
        assert_eq!(
            schema.get("type").and_then(serde_json::Value::as_str),
            Some("integer"),
            "{} serializes as a bare byte, so its schema must be an integer",
            $name
        );

        // A contiguous `minimum..=maximum` cannot express a set with gaps, so a type with
        // gaps says so in its description.
        let description = schema
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("{} has no schema description", $name));
        let lowest_accepted = accepted[0];
        let contiguous =
            accepted.len() == usize::from(highest_accepted) - usize::from(lowest_accepted) + 1;
        if !contiguous {
            assert!(
                description.contains("not contiguous"),
                "{} accepts a set with gaps ({} of {} bytes in range), so its description must \
                 say so: {description:?}",
                $name,
                accepted.len(),
                usize::from(highest_accepted) - usize::from(lowest_accepted) + 1
            );
        }
    }};
}

#[test]
fn every_byte_schema_advertises_the_range_serde_enforces() {
    use uds_protocol::{
        CommunicationControlType, CommunicationType, DataFormatIdentifier,
        DiagnosticSessionType, DtcFormatIdentifier, DtcSettingType, FileOperationMode,
        FunctionalGroupIdentifier, ResetType, SecurityAccessLevel, SecurityAccessType,
        SubnetNumber,
    };

    assert_schema_bound_matches_serde!(
        CommunicationControlType,
        "CommunicationControlType"
    );
    assert_schema_bound_matches_serde!(CommunicationType, "CommunicationType");
    assert_schema_bound_matches_serde!(DataFormatIdentifier, "DataFormatIdentifier");
    assert_schema_bound_matches_serde!(DiagnosticSessionType, "DiagnosticSessionType");
    assert_schema_bound_matches_serde!(DtcFormatIdentifier, "DtcFormatIdentifier");
    assert_schema_bound_matches_serde!(ResetType, "ResetType");
    assert_schema_bound_matches_serde!(DtcSettingType, "DtcSettingType");
    assert_schema_bound_matches_serde!(FileOperationMode, "FileOperationMode");
    assert_schema_bound_matches_serde!(
        FunctionalGroupIdentifier,
        "FunctionalGroupIdentifier"
    );
    assert_schema_bound_matches_serde!(SecurityAccessLevel, "SecurityAccessLevel");
    assert_schema_bound_matches_serde!(SecurityAccessType, "SecurityAccessType");
    assert_schema_bound_matches_serde!(SubnetNumber, "SubnetNumber");
}

#[test]
fn no_schema_description_leaks_a_private_type_or_is_empty() {
    // Types a downstream crate cannot name; a schema that mentions one hands a client
    // author a dead end.
    const PRIVATE: [&str; 2] = ["ZeroSubFunction", "MemoryFormatIdentifier"];

    let json = Api::openapi().to_json().expect("serializes");
    let doc: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    let schemas = doc["components"]["schemas"]
        .as_object()
        .expect("components.schemas");

    for (name, schema) in schemas {
        // Field descriptions are published too, and they come from field rustdoc, so they
        // are just as capable of naming a private type as the top-level one is.
        if let Some(properties) = schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
        {
            for (field, property) in properties {
                let Some(field_description) = property
                    .get("description")
                    .and_then(serde_json::Value::as_str)
                else {
                    continue;
                };
                for private in PRIVATE {
                    assert!(
                        !field_description.contains(private),
                        "{name}.{field}'s published description names the private type \
                         `{private}`: {field_description:?}"
                    );
                }
            }
        }

        let description = schema
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("{name} has no schema description"));

        assert!(
            !description.trim().is_empty(),
            "{name} has an empty schema description"
        );
        for private in PRIVATE {
            assert!(
                !description.contains(private),
                "{name}'s published description names the private type `{private}`: \
                 {description:?}"
            );
        }
        // utoipa drops a `#[doc = concat!(..)]` fragment and leaves the blank line that
        // followed it, so a description starting with whitespace has lost a sentence.
        assert!(
            !description.starts_with(char::is_whitespace),
            "{name}'s description starts with whitespace, which is what a dropped \
             `#[doc = concat!(..)]` fragment leaves behind: {description:?}"
        );
    }
}
