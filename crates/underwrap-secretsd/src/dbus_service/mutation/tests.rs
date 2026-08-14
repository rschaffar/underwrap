use std::collections::HashMap;

use underwrap_secretsd_store::SecretStore;
use zbus::fdo;

use super::{mutate_and_publish, zeroize_item_secrets};
use crate::test_support::{HostResponse, ScriptedPublisher};

#[test]
fn rejected_candidate_does_not_change_live_store() {
    let mut live = SecretStore::new_default();
    live.create_item(
        "default",
        "accepted",
        &HashMap::new(),
        b"synthetic-accepted-secret",
        "text/plain",
        false,
    )
    .unwrap();
    let before = live.clone();
    let script = ScriptedPublisher::new(vec![HostResponse::Reject]);

    let result = mutate_and_publish(&mut live, &script.publisher(), |candidate| {
        candidate.set_collection_label("default", "candidate");
        Ok(())
    });
    let published = script.finish();

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(live, before);
    assert_eq!(
        published[0].section.collections["default"].label,
        "candidate"
    );
}

#[test]
fn fallible_preparation_error_does_not_publish_or_change_live_store() {
    let mut live = SecretStore::new_default();
    let before = live.clone();
    let script = ScriptedPublisher::new(vec![]);

    let result: fdo::Result<()> = mutate_and_publish(&mut live, &script.publisher(), |candidate| {
        candidate.set_collection_label("default", "discarded");
        Err(fdo::Error::Failed("synthetic preparation failure".into()))
    });
    let published = script.finish();

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(live, before);
    assert!(published.is_empty());
}

#[test]
fn item_secret_zeroization_covers_the_complete_store() {
    let mut store = SecretStore::new_default();
    for label in ["first", "second"] {
        store
            .create_item(
                "default",
                label,
                &HashMap::new(),
                label.as_bytes(),
                "text/plain",
                false,
            )
            .unwrap();
    }
    store.create_collection("other", None);
    store
        .create_item(
            "other",
            "third",
            &HashMap::new(),
            b"third",
            "text/plain",
            false,
        )
        .unwrap();

    zeroize_item_secrets(&mut store);

    assert!(
        store
            .collections
            .values()
            .flat_map(|collection| collection.items.values())
            .all(|item| item.secret.iter().all(|byte| *byte == 0))
    );
}
