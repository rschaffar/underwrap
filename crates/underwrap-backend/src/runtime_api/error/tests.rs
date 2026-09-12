use std::fmt;

use super::*;
use crate::test_support::{capture_tracing_output, response_body_json};

const PUBLIC_MESSAGE: &str = "the backend could not complete the request";

#[derive(Debug)]
struct ArbitraryRuntimeSource;

impl fmt::Display for ArbitraryRuntimeSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ARBITRARY-RUNTIME-SOURCE-SENTINEL")
    }
}

fn assert_generic_response_and_log(error: RuntimeError, log: &str, context: &str) {
    assert_eq!(
        response_body_json(error),
        serde_json::json!({
            "__status": 500,
            "code": "internal",
            "message": PUBLIC_MESSAGE,
        }),
    );
    assert!(log.contains("ERROR runtime internal failure"), "{log}");
    assert!(log.contains("response_code=\"internal\""), "{log}");
    assert!(log.contains(&format!("context=\"{context}\"")), "{log}");
    assert!(log.contains("error_type=\""), "{log}");
    assert!(log.contains("ArbitraryRuntimeSource"), "{log}");
    assert!(!log.contains("ARBITRARY-RUNTIME-SOURCE-SENTINEL"), "{log}");
    assert!(!log.contains(" error="), "{log}");
}

#[test]
fn direct_internal_returns_generic_response_and_logs_type_only() {
    let (error, log) = capture_tracing_output(|| {
        RuntimeError::internal(
            "performing direct runtime operation",
            ArbitraryRuntimeSource,
        )
    });

    assert_generic_response_and_log(error, &log, "performing direct runtime operation");
}

#[test]
fn into_internal_adapter_returns_generic_response_and_logs_type_only() {
    let (error, log) = capture_tracing_output(|| {
        Result::<(), _>::Err(ArbitraryRuntimeSource)
            .into_internal("performing adapted runtime operation")
            .unwrap_err()
    });

    assert_generic_response_and_log(error, &log, "performing adapted runtime operation");
}

fn occurrences(sources: &[&str], needle: &str) -> usize {
    sources
        .iter()
        .map(|source| source.matches(needle).count())
        .sum()
}

#[test]
fn production_internal_constructor_inventory_is_static_and_complete() {
    let sources = [
        include_str!("../mod.rs"),
        include_str!("../invocations.rs"),
        include_str!("../wrappers.rs"),
    ];

    let adapter_calls = occurrences(&sources, ".into_internal(");
    let direct_calls = occurrences(&sources, "RuntimeError::internal(");

    // Resource and pair lookup have separate static contexts; both retain
    // the type-only internal-error adapter, never arbitrary source text.
    assert_eq!(adapter_calls, 18);
    assert_eq!(direct_calls, 2);
    assert_eq!(adapter_calls + direct_calls, 20);
}
