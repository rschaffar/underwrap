use super::*;
use crate::test_support::{HostResponse, ObjectServerHarness, ScriptedPublisher};

#[test]
fn reconciliation_failure_stops_private_service() {
    const CHILD: &str = "UNDERWRAP_TEST_EXPORT_FAILURE_CHILD";
    if let Ok(mode) = std::env::var(CHILD) {
        if mode == "cancel" {
            drop(ReconcileGuard::new());
        } else {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let store = Arc::new(RwLock::new(SecretStore::new_default()));
                    let script = ScriptedPublisher::new(vec![HostResponse::Ack]);
                    let server = ObjectServerHarness::new().await;
                    // An unexpected existing interface forces at() == false after
                    // candidate ACK/commit. A registration failure must not return
                    // success, retract the accepted store, or continue serving.
                    server
                        .object_server()
                        .at(
                            collection_path("collision").unwrap(),
                            CollectionInterface::new(
                                "default".into(),
                                store.clone(),
                                script.publisher(),
                            ),
                        )
                        .await
                        .unwrap();
                    super::super::mutation::mutate_and_export(
                        &store,
                        &script.publisher(),
                        &server.object_server(),
                        |candidate| {
                            candidate.create_collection("collision", None);
                            Ok(())
                        },
                    )
                    .await
                    .unwrap();
                });
        }
        panic!("failed or cancelled reconciliation returned");
    }
    for mode in ["collision", "cancel"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "dbus_service::exports::tests::reconciliation_failure_stops_private_service",
                "--nocapture",
            ])
            .env(CHILD, mode)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("accepted state export reconciliation failed; stopping")
        );
    }
}
