//! Drift detection for `crates/backend-backendd/openapi.json`.
//!
//! `utoipa` derives the spec from handler annotations and DTO
//! `ToSchema` impls; this test serializes the live `ApiDoc` to pretty
//! JSON and compares it to the committed file. If anything drifts the
//! file is rewritten in place and the test fails with a one-line
//! instruction so the dev only has to `git add` the result.
//!
//! Adding a new admin endpoint or DTO means: annotate with
//! `#[utoipa::path(...)]` / `#[derive(utoipa::ToSchema)]`, register the
//! handler in `admin_api::admin_router` via `OpenApiRouter::routes`,
//! then run `cargo test --workspace` once to refresh `openapi.json`
//! and commit the result.

use std::path::PathBuf;

use underwrap_backendd::admin_openapi_spec;

fn openapi_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("openapi.json")
}

#[test]
fn openapi_spec_round_trips() {
    let doc = admin_openapi_spec();
    let serialized = serde_json::to_string_pretty(&doc).expect("OpenApi serializes to JSON");
    // Trailing newline so editors / pre-commit hooks don't fight over it.
    let expected = format!("{serialized}\n");

    let path = openapi_path();
    let actual = std::fs::read_to_string(&path).unwrap_or_default();
    if actual != expected {
        std::fs::write(&path, &expected).expect("write openapi.json");
        panic!(
            "`{}` was out of sync with the live ApiDoc; it has been \
             rewritten. Commit the updated file.",
            path.display()
        );
    }
}
