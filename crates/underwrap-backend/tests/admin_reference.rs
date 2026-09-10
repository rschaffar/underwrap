//! Documentation regressions against the separately tested handler contracts.
//! These assertions check generated meaning, not runtime atomicity or erasure.

use serde_json::Value;
use underwrap_backend::admin_openapi_spec;

fn spec() -> Value {
    serde_json::to_value(admin_openapi_spec()).unwrap()
}

fn operation<'a>(spec: &'a Value, path: &str, method: &str) -> &'a Value {
    spec["paths"][path]
        .get(method)
        .expect("documented operation")
}

#[test]
fn delete_guards_and_optional_force_match_handlers() {
    let spec = spec();
    for path in ["/v1/admin/stores/{id}", "/v1/admin/users/{id}"] {
        let responses = &operation(&spec, path, "delete")["responses"];
        assert!(responses.get("409").is_some(), "{path}");
        assert!(responses.get("400").is_none(), "{path}");
    }
    let parameters = operation(&spec, "/v1/admin/stores/{id}", "delete")["parameters"]
        .as_array()
        .unwrap();
    let force = parameters.iter().find(|p| p["name"] == "force").unwrap();
    assert_eq!(force["required"], false);
    assert!(
        force["description"]
            .as_str()
            .unwrap()
            .contains("defaults to false")
    );
}

#[test]
fn flat_slots_and_absent_keys_are_not_confused_with_missing_resources() {
    let spec = spec();
    let path = "/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}";
    for (path, method) in [
        (path, "get"),
        (path, "put"),
        (path, "delete"),
        (
            "/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}/rename",
            "post",
        ),
    ] {
        let op = operation(&spec, path, method);
        let slot = op["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "slot")
            .unwrap();
        assert!(
            slot["description"]
                .as_str()
                .unwrap()
                .starts_with("Flat slot: env or files.")
        );
        assert!(op["responses"].get("400").is_some());
    }
    let read = &operation(&spec, path, "get")["responses"];
    assert!(
        read["200"]["description"]
            .as_str()
            .unwrap()
            .contains("material_b64: null")
    );
    assert_eq!(read["404"]["description"], "Store or resource not found.");
    let delete = &operation(&spec, path, "delete")["responses"];
    assert!(
        delete["200"]["description"]
            .as_str()
            .unwrap()
            .contains("existed is false")
    );
    assert_eq!(delete["404"]["description"], "Store or resource not found.");
}

#[test]
fn mutation_descriptions_preserve_concurrency_and_key_lifetime_limits() {
    let spec = spec();
    let schemas = &spec["components"]["schemas"];
    assert!(
        schemas["AdminPairWithNewResourceRequest"]["description"]
            .as_str()
            .unwrap()
            .contains("not transactional")
    );
    assert!(
        schemas["AdminSecretRenameRequest"]["description"]
            .as_str()
            .unwrap()
            .contains("last-writer-wins")
    );
    assert!(
        schemas["AdminStoreUsageSummary"]["properties"]["has_secrets"]["description"]
            .as_str()
            .unwrap()
            .contains("unavailable inspection")
    );
    assert!(
        schemas["AdminStoreBackendKind"]["description"]
            .as_str()
            .unwrap()
            .contains("does not expose a separate store-detail endpoint")
    );
    let lock = operation(&spec, "/v1/admin/stores/{id}/lock", "post");
    assert!(
        lock["responses"]["200"]["description"]
            .as_str()
            .unwrap()
            .contains("final owner is dropped")
    );
    for method in ["put", "delete"] {
        let op = operation(&spec, "/v1/admin/client-instances/{id}", method);
        assert!(
            op["responses"]["200"]["description"]
                .as_str()
                .unwrap()
                .contains("best effort")
        );
        assert!(
            op["responses"]["200"]["description"]
                .as_str()
                .unwrap()
                .contains("success does not certify")
        );
    }
}
