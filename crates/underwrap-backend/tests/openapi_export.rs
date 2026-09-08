//! Semantic contract checks and byte-drift detection for the admin OpenAPI.
//!
//! `utoipa` derives the spec from handler annotations and DTO `ToSchema`
//! implementations. The semantic tests below pin the security and lifecycle
//! meaning consumed by clients. The final test serializes that same document,
//! compares it to the committed file, and rewrites the file when bytes drift.
//!
//! Adding a new admin endpoint or DTO means: annotate it with
//! `#[utoipa::path(...)]` / `#[derive(utoipa::ToSchema)]`, register the handler
//! through `OpenApiRouter::routes`, then run this test once to refresh
//! `openapi.json` and commit the result.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::{Value, json};
use underwrap_backend::admin_openapi_spec;

const HTTP_METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];
const SERVICE_ERROR_SCHEMA: &str = "#/components/schemas/ServiceErrorResponse";
const STORE_LOCKED_DESCRIPTION: &str = "Store is unavailable (`store_locked` or `store_busy`).";
const STORE_BUSY_DESCRIPTION: &str = "Store is busy (`store_busy`).";

const PUBLIC_OPERATIONS: [&str; 3] = [
    "GET /v1/admin/auth/bootstrap/status",
    "GET /v1/admin/auth/whoami",
    "POST /v1/admin/auth/login",
];

const PROTECTED_MUTATIONS: [&str; 34] = [
    "POST /v1/admin/auth/logout",
    "POST /v1/admin/auth/password",
    "POST /v1/admin/client-instances",
    "POST /v1/admin/store-keys",
    "PUT /v1/admin/client-instances/{id}",
    "DELETE /v1/admin/client-instances/{id}",
    "POST /v1/admin/stores",
    "PUT /v1/admin/stores/{id}",
    "DELETE /v1/admin/stores/{id}",
    "POST /v1/admin/stores/{id}/lock",
    "POST /v1/admin/stores/{id}/migrate",
    "POST /v1/admin/stores/{id}/rotate-key",
    "POST /v1/admin/stores/{id}/unlock",
    "POST /v1/admin/stores/{id}/pairs",
    "POST /v1/admin/stores/{id}/pairs/with-new-resource",
    "DELETE /v1/admin/stores/{id}/pairs/{wrapper}/{rid}",
    "POST /v1/admin/stores/{id}/resources",
    "PUT /v1/admin/stores/{id}/resources/{rid}",
    "DELETE /v1/admin/stores/{id}/resources/{rid}",
    "PUT /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}",
    "DELETE /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}",
    "POST /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}/rename",
    "PUT /v1/admin/stores/{id}/wrapper-permissions/{client_instance}/{wrapper}/{rid}",
    "DELETE /v1/admin/stores/{id}/wrapper-permissions/{client_instance}/{wrapper}/{rid}",
    "PUT /v1/admin/stores/{sid}/resources/{rid}/libsecret/aliases/{name}",
    "DELETE /v1/admin/stores/{sid}/resources/{rid}/libsecret/aliases/{name}",
    "POST /v1/admin/stores/{sid}/resources/{rid}/libsecret/collections",
    "DELETE /v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}",
    "POST /v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}/items",
    "DELETE /v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}/items/{iid}",
    "POST /v1/admin/users",
    "PUT /v1/admin/users/{id}",
    "DELETE /v1/admin/users/{id}",
    "POST /v1/admin/users/{id}/password",
];

const STORE_LOCKED_OPERATIONS: [&str; 28] = [
    "GET /v1/admin/stores/{id}/pairs",
    "POST /v1/admin/stores/{id}/pairs",
    "POST /v1/admin/stores/{id}/pairs/with-new-resource",
    "DELETE /v1/admin/stores/{id}/pairs/{wrapper}/{rid}",
    "GET /v1/admin/stores/{id}/resources",
    "POST /v1/admin/stores/{id}/resources",
    "PUT /v1/admin/stores/{id}/resources/{rid}",
    "DELETE /v1/admin/stores/{id}/resources/{rid}",
    "POST /v1/admin/stores/{id}/migrate",
    "POST /v1/admin/stores/{id}/rotate-key",
    "GET /v1/admin/stores/{id}/secrets",
    "GET /v1/admin/stores/{id}/secrets-view",
    "GET /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}",
    "PUT /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}",
    "DELETE /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}",
    "POST /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}/rename",
    "GET /v1/admin/stores/{id}/wrapper-permissions",
    "GET /v1/admin/stores/{id}/wrapper-permissions-view",
    "PUT /v1/admin/stores/{id}/wrapper-permissions/{client_instance}/{wrapper}/{rid}",
    "DELETE /v1/admin/stores/{id}/wrapper-permissions/{client_instance}/{wrapper}/{rid}",
    "GET /v1/admin/stores/{sid}/resources/{rid}/libsecret",
    "PUT /v1/admin/stores/{sid}/resources/{rid}/libsecret/aliases/{name}",
    "DELETE /v1/admin/stores/{sid}/resources/{rid}/libsecret/aliases/{name}",
    "POST /v1/admin/stores/{sid}/resources/{rid}/libsecret/collections",
    "DELETE /v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}",
    "POST /v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}/items",
    "DELETE /v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}/items/{iid}",
    "GET /v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}/items/{iid}/secret",
];

const STORE_BUSY_ONLY_OPERATIONS: [&str; 4] = [
    "PUT /v1/admin/stores/{id}",
    "DELETE /v1/admin/stores/{id}",
    "POST /v1/admin/stores/{id}/lock",
    "POST /v1/admin/stores/{id}/unlock",
];

const MIGRATION_RECOVERY_OPERATIONS: [&str; 32] = [
    "PUT /v1/admin/stores/{id}",
    "DELETE /v1/admin/stores/{id}",
    "POST /v1/admin/stores/{id}/lock",
    "POST /v1/admin/stores/{id}/migrate",
    "POST /v1/admin/stores/{id}/rotate-key",
    "POST /v1/admin/stores/{id}/unlock",
    "GET /v1/admin/stores/{id}/pairs",
    "POST /v1/admin/stores/{id}/pairs",
    "POST /v1/admin/stores/{id}/pairs/with-new-resource",
    "DELETE /v1/admin/stores/{id}/pairs/{wrapper}/{rid}",
    "GET /v1/admin/stores/{id}/resources",
    "POST /v1/admin/stores/{id}/resources",
    "PUT /v1/admin/stores/{id}/resources/{rid}",
    "DELETE /v1/admin/stores/{id}/resources/{rid}",
    "GET /v1/admin/stores/{id}/secrets",
    "GET /v1/admin/stores/{id}/secrets-view",
    "GET /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}",
    "PUT /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}",
    "DELETE /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}",
    "POST /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}/rename",
    "GET /v1/admin/stores/{id}/wrapper-permissions",
    "GET /v1/admin/stores/{id}/wrapper-permissions-view",
    "PUT /v1/admin/stores/{id}/wrapper-permissions/{client_instance}/{wrapper}/{rid}",
    "DELETE /v1/admin/stores/{id}/wrapper-permissions/{client_instance}/{wrapper}/{rid}",
    "GET /v1/admin/stores/{sid}/resources/{rid}/libsecret",
    "PUT /v1/admin/stores/{sid}/resources/{rid}/libsecret/aliases/{name}",
    "DELETE /v1/admin/stores/{sid}/resources/{rid}/libsecret/aliases/{name}",
    "POST /v1/admin/stores/{sid}/resources/{rid}/libsecret/collections",
    "DELETE /v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}",
    "POST /v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}/items",
    "DELETE /v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}/items/{iid}",
    "GET /v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}/items/{iid}/secret",
];

const LOCK_RESPONSES: [(&str, &str); 5] = [
    ("200", "Store locked; in-memory key material wiped."),
    ("400", "Store is not encrypted (`store_not_encrypted`)."),
    ("404", "Store not found (`not_found`)."),
    (
        "500",
        "Trusted-host migration recovery is required (`migration_recovery_required`).",
    ),
    ("503", STORE_BUSY_DESCRIPTION),
];

const ROTATE_RESPONSES: [(&str, &str); 5] = [
    (
        "200",
        "Key rotated and returned once; the displaced encrypted generation is retained for backend-host recovery.",
    ),
    ("404", "Store not found (`not_found`)."),
    ("409", "Store is not encrypted (`not_encrypted`)."),
    (
        "500",
        "Rotation failed (`rotate_stage_failed`, `rotate_list_failed`, `rotate_canary_failed`, `rotate_read_failed`, `rotate_write_failed`, `rotate_verify_failed`, `rotate_swap_failed`, `rotate_reopen_failed`, `rotation_recovery_required`, or `migration_recovery_required`).",
    ),
    ("503", STORE_LOCKED_DESCRIPTION),
];

const MIGRATE_RESPONSES: [(&str, &str); 5] = [
    ("200", "Store migrated at its stable id."),
    (
        "400",
        "Request extraction or direction-specific key validation failed (`invalid_request`, `key_required`, `unexpected_key`, or `invalid_key_hex`).",
    ),
    ("404", "Store not found (`not_found`)."),
    (
        "409",
        "Requested backend matches the current backend (`same_backend`).",
    ),
    (
        "500",
        "Migration failed (`migrate_stage_failed`, `migrate_list_failed`, `migrate_canary_failed`, `migrate_read_failed`, `migrate_write_failed`, `migrate_verify_failed`, `migrate_swap_failed`, `migrate_reopen_failed`, `write_store_registry_failed`, or `migration_recovery_required`).",
    ),
];

const UNLOCK_RESPONSES: [(&str, &str); 7] = [
    ("200", "Store unlocked."),
    (
        "400",
        "Key hex is invalid (`invalid_key_hex`) or the store is not encrypted (`store_not_encrypted`).",
    ),
    (
        "403",
        "Key does not match the encrypted store (`key_mismatch`).",
    ),
    ("404", "Store not found (`not_found`)."),
    ("409", "Store is already unlocked (`already_unlocked`)."),
    (
        "500",
        "Opening the store failed (`open_store_failed`), or trusted-host rotation or migration recovery is required (`rotation_recovery_required` or `migration_recovery_required`).",
    ),
    ("503", STORE_BUSY_DESCRIPTION),
];

fn openapi_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("openapi.json")
}

fn spec_json() -> Value {
    serde_json::to_value(admin_openapi_spec()).expect("OpenApi serializes to JSON")
}

fn operations(spec: &Value) -> Vec<(&str, &str, &Value)> {
    let paths = spec["paths"]
        .as_object()
        .expect("OpenApi paths is an object");
    let mut operations = Vec::new();
    for (path, path_item) in paths {
        let path_item = path_item.as_object().expect("path item is an object");
        for method in HTTP_METHODS {
            if let Some(operation) = path_item.get(method) {
                operations.push((method, path.as_str(), operation));
            }
        }
    }
    operations
}

fn operation_id(method: &str, path: &str) -> String {
    format!("{} {path}", method.to_ascii_uppercase())
}

fn expected_set(operations: &[&str]) -> BTreeSet<String> {
    operations
        .iter()
        .map(|operation| (*operation).into())
        .collect()
}

fn operation<'a>(spec: &'a Value, id: &str) -> &'a Value {
    let (method, path) = id
        .split_once(' ')
        .expect("operation id has method and path");
    let method = method.to_ascii_lowercase();
    spec["paths"][path]
        .get(method.as_str())
        .unwrap_or_else(|| panic!("missing operation {id}"))
}

fn response_schema(response: &Value) -> Option<&str> {
    response
        .pointer("/content/application~1json/schema/$ref")
        .and_then(Value::as_str)
}

fn effective_security<'a>(spec: &'a Value, operation: &'a Value) -> Option<&'a Value> {
    operation.get("security").or_else(|| spec.get("security"))
}

fn is_public_security(security: Option<&Value>) -> bool {
    security.is_none_or(|requirements| {
        requirements
            .as_array()
            .is_some_and(|requirements| requirements.is_empty())
    })
}

#[test]
fn security_requirements_match_the_live_router_contract() {
    let spec = spec_json();
    let root_security = spec.get("security");
    assert!(
        is_public_security(root_security),
        "OpenAPI root security must be absent or an empty requirement array"
    );

    let operations = operations(&spec);
    let expected_public = expected_set(&PUBLIC_OPERATIONS);
    let actual_public = operations
        .iter()
        .filter(|(_, _, operation)| is_public_security(effective_security(&spec, operation)))
        .map(|(method, path, _)| operation_id(method, path))
        .collect::<BTreeSet<_>>();
    assert_eq!(actual_public, expected_public);

    let session_only = json!([{"session_cookie": []}]);
    for (method, path, operation) in &operations {
        if *method == "get" && !expected_public.contains(&operation_id(method, path)) {
            assert_eq!(
                effective_security(&spec, operation),
                Some(&session_only),
                "{method} {path} must require only the session cookie"
            );
        }
    }

    let expected_protected = expected_set(&PROTECTED_MUTATIONS);
    let actual_protected = operations
        .iter()
        .filter(|(method, path, _)| {
            *method != "get" && operation_id(method, path) != "POST /v1/admin/auth/login"
        })
        .map(|(method, path, _)| operation_id(method, path))
        .collect::<BTreeSet<_>>();
    assert_eq!(actual_protected, expected_protected);
    assert_eq!(actual_protected.len(), 34);

    let session_and_csrf = json!([{"session_cookie": [], "csrf_header": []}]);
    for id in &expected_protected {
        assert_eq!(
            effective_security(&spec, operation(&spec, id)),
            Some(&session_and_csrf),
            "{id} must express session and CSRF in one requirement object"
        );
    }
}

#[test]
fn security_scheme_definitions_are_exact() {
    let spec = spec_json();
    let expected = json!({
        "csrf_header": {
            "type": "apiKey",
            "in": "header",
            "name": "X-CSRF-Token"
        },
        "session_cookie": {
            "type": "apiKey",
            "in": "cookie",
            "name": "__Host-underwrap-session"
        }
    });
    assert_eq!(spec.pointer("/components/securitySchemes"), Some(&expected));
}

#[test]
fn store_unavailable_responses_are_exact_and_use_503() {
    let spec = spec_json();
    let expected_locked = expected_set(&STORE_LOCKED_OPERATIONS);
    let expected_busy = expected_set(&STORE_BUSY_ONLY_OPERATIONS);
    let expected_unavailable = expected_locked
        .union(&expected_busy)
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut operations_with_503 = BTreeSet::new();

    for (method, path, operation) in operations(&spec) {
        let responses = operation["responses"]
            .as_object()
            .expect("operation responses is an object");
        assert!(
            !responses.contains_key("423"),
            "{} unexpectedly advertises HTTP 423",
            operation_id(method, path)
        );
        if responses.contains_key("503") {
            operations_with_503.insert(operation_id(method, path));
        }
    }

    assert_eq!(operations_with_503, expected_unavailable);
    assert_eq!(operations_with_503.len(), 32);
    for (operations, description) in [
        (&expected_locked, STORE_LOCKED_DESCRIPTION),
        (&expected_busy, STORE_BUSY_DESCRIPTION),
    ] {
        for id in operations {
            let response = &operation(&spec, id)["responses"]["503"];
            assert_eq!(response["description"], description, "{id}");
            assert_eq!(
                response_schema(response),
                Some(SERVICE_ERROR_SCHEMA),
                "{id}"
            );
        }
    }
}

#[test]
fn migration_recovery_response_inventory_is_exact() {
    let spec = spec_json();
    let expected = expected_set(&MIGRATION_RECOVERY_OPERATIONS);
    let actual = operations(&spec)
        .into_iter()
        .filter(|(_, _, operation)| {
            operation["responses"]
                .as_object()
                .and_then(|responses| responses.get("500"))
                .and_then(|response| response["description"].as_str())
                .is_some_and(|description| description.contains("migration_recovery_required"))
        })
        .map(|(method, path, _)| operation_id(method, path))
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 32);
    for id in expected {
        assert_eq!(
            response_schema(&operation(&spec, &id)["responses"]["500"]),
            Some(SERVICE_ERROR_SCHEMA),
            "{id}"
        );
    }
}

#[test]
fn lifecycle_response_contracts_are_exact() {
    let spec = spec_json();
    assert_response_contract(&spec, "POST /v1/admin/stores/{id}/lock", &LOCK_RESPONSES);
    assert_response_contract(
        &spec,
        "POST /v1/admin/stores/{id}/rotate-key",
        &ROTATE_RESPONSES,
    );
    let migrate = operation(&spec, "POST /v1/admin/stores/{id}/migrate");
    let migrate_without_503 = MIGRATE_RESPONSES;
    for (status, description) in migrate_without_503 {
        assert_eq!(migrate["responses"][status]["description"], description);
    }
    assert_eq!(
        migrate["responses"]["503"]["description"],
        STORE_LOCKED_DESCRIPTION
    );
    assert_response_contract(
        &spec,
        "POST /v1/admin/stores/{id}/unlock",
        &UNLOCK_RESPONSES,
    );
}

#[test]
fn store_key_and_lifecycle_schemas_match_the_admin_contract() {
    let spec = spec_json();
    let key_operation = operation(&spec, "POST /v1/admin/store-keys");
    assert_eq!(
        key_operation["responses"]["200"]["description"],
        "Fresh key generated without mutating storage; response carries exact no-cache headers."
    );
    let headers = key_operation["responses"]["200"]["headers"]
        .as_object()
        .unwrap();
    assert_eq!(
        headers.keys().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from(["Cache-Control".into(), "Expires".into(), "Pragma".into()])
    );
    for (id, operation_text) in [
        (
            "POST /v1/admin/stores",
            "Encrypted creation requires `key_hex` as exactly 64 lowercase hexadecimal characters without trimming or normalization; plain creation requires it to be omitted. Explicit JSON `null` is invalid for every backend kind.",
        ),
        (
            "POST /v1/admin/stores/{id}/migrate",
            "Migration to encrypted requires `key_hex` as exactly 64 lowercase hexadecimal characters without trimming or normalization; migration to plain requires it to be omitted. Explicit JSON `null` is invalid for every migration direction.",
        ),
    ] {
        assert!(
            operation(&spec, id)["description"]
                .as_str()
                .is_some_and(|description| description.contains(operation_text)),
            "{id}"
        );
    }
    for schema in ["AdminStoreCreateRequest", "AdminStoreMigrateRequest"] {
        let schema = &spec["components"]["schemas"][schema];
        assert!(
            schema["description"]
                .as_str()
                .is_some_and(|description| description.contains("requires `key_hex`"))
        );
        assert!(
            !schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("key_hex"))
        );
        let key_hex = &schema["properties"]["key_hex"];
        assert_eq!(key_hex["type"], "string");
        assert!(key_hex.get("oneOf").is_none());
        assert!(key_hex["description"].as_str().is_some_and(|description| {
            description
                .contains("Required exactly when `backend` is `encrypted`; otherwise omitted.")
        }));
    }
    let lifecycle = &spec["components"]["schemas"]["AdminStoreLifecycleState"]["enum"];
    assert_eq!(
        lifecycle,
        &json!(["ready", "unlocked", "locked", "busy", "recovery-required"])
    );
    let item = &spec["components"]["schemas"]["AdminStoreListItem"];
    assert!(
        item["required"]
            .as_array()
            .unwrap()
            .contains(&json!("lifecycle_state"))
    );
    assert!(item["properties"].get("locked").is_none());
    let actions = &spec["components"]["schemas"]["AdminStoreActionState"];
    assert!(
        actions["required"]
            .as_array()
            .unwrap()
            .contains(&json!("can_update"))
    );
    assert!(
        actions["required"]
            .as_array()
            .unwrap()
            .contains(&json!("can_migrate"))
    );
}

fn assert_response_contract(spec: &Value, id: &str, expected: &[(&str, &str)]) {
    let responses = operation(spec, id)["responses"]
        .as_object()
        .expect("operation responses is an object");
    let actual_statuses = responses.keys().cloned().collect::<BTreeSet<_>>();
    let expected_statuses = expected
        .iter()
        .map(|(status, _)| (*status).to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual_statuses, expected_statuses, "{id}");

    for (status, description) in expected {
        let response = &responses[*status];
        assert_eq!(
            response["description"], *description,
            "{id} response {status}"
        );
        if *status != "200" {
            assert_eq!(
                response_schema(response),
                Some(SERVICE_ERROR_SCHEMA),
                "{id} response {status}"
            );
        }
    }
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
