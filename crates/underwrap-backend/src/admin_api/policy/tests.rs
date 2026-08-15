use std::sync::Arc;

use super::*;
use crate::test_support::{admin_principal, capture_tracing_output, crud_state};

fn capture(run: impl FnOnce()) -> String {
    capture_tracing_output(run).1
}

#[test]
fn drop_client_instance_permissions_on_locked_store_warns_and_audits() {
    use underwrap_protocol::service::{AdminStoreBackendKind, AdminStoreCreateRequest};
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let store_id = crate::test_support::create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );

    // Force the registry into the Locked state for this store id.
    state.shared.stores.write().unwrap().lock(&store_id);

    let out = capture(|| {
        drop_client_instance_permissions(&state, &p, &store_id, "inst_xyz")
            .expect("locked store must not propagate Err");
    });

    // Both the structured tracing::warn! and the
    // log_admin_event line must fire so operators can find the
    // orphan rows.
    assert!(
        out.contains("store locked"),
        "expected the warn! line, got: {out}",
    );
    assert!(
        out.contains("action=client-instance.orphan-permissions"),
        "expected the audit event, got: {out}",
    );
    assert!(
        out.contains("resource=Some(\"client-instance:inst_xyz\")"),
        "audit must name the orphaned client instance, got: {out}",
    );
    assert!(
        out.contains("operation=Some(\"store-locked\")"),
        "audit must surface the store-locked reason, got: {out}",
    );
    assert!(
        out.contains("result=skipped"),
        "audit must report result=skipped, got: {out}",
    );
}

#[test]
fn drop_client_instance_permissions_on_missing_store_is_a_quiet_noop() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();

    let out = capture(|| {
        drop_client_instance_permissions(&state, &p, "store-that-never-existed", "inst_xyz")
            .expect("missing store must not propagate Err");
    });

    // Missing-store cascade is info-level only; no audit line.
    assert!(
        !out.contains("action=client-instance.orphan-permissions"),
        "missing-store path must not emit the orphan audit event, got: {out}",
    );
}
