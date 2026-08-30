use std::fmt;

use super::*;
use crate::admin_api::util::IntoInternal as _;
use crate::test_support::{capture_tracing_output, response_body_json};

const PUBLIC_MESSAGE: &str = "the backend could not complete the request";

#[derive(Debug)]
struct ArbitrarySource;

impl fmt::Display for ArbitrarySource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ARBITRARY-SOURCE-SENTINEL")
    }
}

fn assert_generic_envelope(error: AdminError) {
    assert_eq!(
        response_body_json(error),
        serde_json::json!({
            "__status": 500,
            "code": "internal",
            "message": PUBLIC_MESSAGE,
        }),
    );
}

fn assert_generic_log(log: &str, context: &str) {
    assert!(log.contains("ERROR admin internal failure"), "{log}");
    assert!(log.contains("response_code=\"internal\""), "{log}");
    assert!(log.contains(&format!("context=\"{context}\"")), "{log}");
    assert!(log.contains("error_type=\""), "{log}");
    assert!(!log.contains("ARBITRARY-SOURCE-SENTINEL"), "{log}");
    assert!(!log.contains(" error="), "{log}");
}

#[test]
fn direct_internal_returns_generic_response_and_logs_type_only() {
    let (error, log) = capture_tracing_output(|| {
        AdminError::internal("performing direct admin operation", ArbitrarySource)
    });

    assert_generic_envelope(error);
    assert_generic_log(&log, "performing direct admin operation");
    assert!(log.contains("ArbitrarySource"), "{log}");
}

#[test]
fn into_internal_adapter_returns_generic_response_and_logs_type_only() {
    let (error, log) = capture_tracing_output(|| {
        Result::<(), _>::Err(ArbitrarySource)
            .into_internal("performing adapted admin operation")
            .unwrap_err()
    });

    assert_generic_envelope(error);
    assert_generic_log(&log, "performing adapted admin operation");
    assert!(log.contains("ArbitrarySource"), "{log}");
}

#[test]
fn internal_with_source_keeps_chain_and_path_only_in_audited_log() {
    let source = anyhow::anyhow!("leaf failure at /srv/underwrap/registry")
        .context("outer registry failure");
    let (error, log) = capture_tracing_output(|| {
        AdminError::internal_with_source(
            "write_store_registry_failed",
            "could not persist store registry",
            "persisting tested store registry record",
            source,
        )
    });
    let response = response_body_json(error);

    assert_eq!(
        response,
        serde_json::json!({
            "__status": 500,
            "code": "write_store_registry_failed",
            "message": "could not persist store registry",
        }),
    );
    let response_text = response.to_string();
    assert!(!response_text.contains("outer registry failure"));
    assert!(!response_text.contains("/srv/underwrap/registry"));
    assert!(log.contains("ERROR admin internal failure"), "{log}");
    assert!(
        log.contains("response_code=\"write_store_registry_failed\""),
        "{log}",
    );
    assert!(
        log.contains("context=\"persisting tested store registry record\""),
        "{log}",
    );
    assert!(log.contains("error_type=\"anyhow::Error\""), "{log}");
    assert!(
        log.contains("error=outer registry failure: leaf failure at /srv/underwrap/registry"),
        "{log}"
    );
}

#[test]
fn source_less_internal_with_code_emits_no_diagnostic_event() {
    let (error, log) = capture_tracing_output(|| {
        AdminError::internal_with_code("fixed_failure", "fixed safe public message")
    });

    assert_eq!(
        response_body_json(error),
        serde_json::json!({
            "__status": 500,
            "code": "fixed_failure",
            "message": "fixed safe public message",
        }),
    );
    assert!(log.is_empty(), "{log}");
}

fn occurrences(sources: &[&str], needle: &str) -> usize {
    sources
        .iter()
        .map(|source| source.matches(needle).count())
        .sum()
}

fn without_whitespace(source: &str) -> String {
    source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

#[test]
fn production_internal_constructor_inventory_is_static_and_complete() {
    let generic_adapter_sources = [
        include_str!("../secrets.rs"),
        include_str!("../policy.rs"),
        include_str!("../pairs.rs"),
        include_str!("../references.rs"),
        include_str!("../views.rs"),
        include_str!("../libsecret.rs"),
        include_str!("../resources.rs"),
    ];
    let generic_direct_sources = [
        include_str!("../policy.rs"),
        include_str!("../pairs.rs"),
        include_str!("../users.rs"),
    ];
    let migrated_sources = [
        include_str!("../stores/mod.rs"),
        include_str!("../stores/crud.rs"),
        include_str!("../client_instances.rs"),
    ];
    let source_less_sources = [
        include_str!("../csrf.rs"),
        include_str!("../stores/lifecycle.rs"),
        include_str!("../stores/crud.rs"),
        include_str!("../client_instances.rs"),
        include_str!("../stores/lifecycle/rotation.rs"),
        include_str!("../users.rs"),
        include_str!("../auth/mod.rs"),
    ];
    let error_source = include_str!("../error.rs");

    let generic_paths = occurrences(&generic_adapter_sources, ".into_internal(")
        + occurrences(&generic_direct_sources, "AdminError::internal(")
        + occurrences(
            &[
                include_str!("../secrets.rs"),
                include_str!("../libsecret.rs"),
            ],
            "AdminError::from_resource_write(",
        );
    assert_eq!(generic_paths, 44);
    assert_eq!(
        occurrences(&generic_adapter_sources, "selecting flat resource slot"),
        1,
    );
    assert_eq!(
        occurrences(&migrated_sources, "AdminError::internal_with_source("),
        13,
    );

    let list_mappings = [
        (
            "list_store_failed",
            "could not inspect store contents",
            "checking store contents for management view",
        ),
        (
            "list_store_failed",
            "could not inspect store contents",
            "checking store contents before delete",
        ),
    ];
    let create_dir_mappings = [
        (
            "create_data_dir_failed",
            "could not create store data directory",
            "creating plain store data directory",
        ),
        (
            "create_data_dir_failed",
            "could not create store data directory",
            "creating encrypted stores data root",
        ),
    ];
    let open_mappings = [
        (
            "open_store_failed",
            "could not open store",
            "opening plain store after creation",
        ),
        (
            "open_store_failed",
            "could not open store",
            "creating encrypted store backend",
        ),
    ];
    let canary_mappings = [(
        "canary_write_failed",
        "could not initialize encrypted store",
        "writing unlock canary for new encrypted store",
    )];
    let store_registry_mappings = [
        (
            "write_store_registry_failed",
            "could not persist store registry",
            "persisting created store registry record",
        ),
        (
            "write_store_registry_failed",
            "could not persist store registry",
            "persisting updated store registry record",
        ),
        (
            "write_store_registry_failed",
            "could not persist store registry",
            "deleting store registry record",
        ),
    ];
    let client_registry_mappings = [
        (
            "write_client_instances_failed",
            "could not persist client instance registry",
            "persisting created client instance registry record",
        ),
        (
            "write_client_instances_failed",
            "could not persist client instance registry",
            "persisting updated client instance registry record",
        ),
        (
            "write_client_instances_failed",
            "could not persist client instance registry",
            "deleting client instance registry record",
        ),
    ];
    let categories = [
        ("list", list_mappings.as_slice(), 2),
        ("create-dir", create_dir_mappings.as_slice(), 2),
        ("open", open_mappings.as_slice(), 2),
        ("canary", canary_mappings.as_slice(), 1),
        ("store-registry", store_registry_mappings.as_slice(), 3),
        ("client-registry", client_registry_mappings.as_slice(), 3),
    ];
    let normalized_sources = without_whitespace(&migrated_sources.concat());
    let mut exact_calls = std::collections::HashSet::new();

    for (category, mappings, expected_count) in categories {
        assert_eq!(mappings.len(), expected_count, "{category}");
        for (code, public_message, context) in mappings {
            let exact_call = without_whitespace(&format!(
                "AdminError::internal_with_source(\"{code}\",\"{public_message}\",\"{context}\",e,)"
            ));
            assert_eq!(
                normalized_sources.matches(&exact_call).count(),
                1,
                "{category}: {code} / {public_message} / {context}",
            );
            assert!(
                exact_calls.insert(exact_call),
                "duplicate {category} mapping"
            );
        }
    }
    assert_eq!(exact_calls.len(), 13);

    assert_eq!(
        occurrences(&source_less_sources, "AdminError::internal_with_code("),
        17,
    );

    assert!(error_source.contains("code: &'static str"));
    assert!(error_source.contains("public_message: &'static str"));
    assert!(error_source.contains("context: &'static str"));
    assert!(!error_source.contains("format!(\"{context}: {error:#}\")"));
}
