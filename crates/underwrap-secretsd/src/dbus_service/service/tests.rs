use super::*;
use crate::test_support::{HostResponse, ObjectServerHarness, service_with_script};
use zbus::zvariant::Str;

fn collection_properties(label: &str) -> HashMap<String, OwnedValue> {
    HashMap::from([(
        "org.freedesktop.Secret.Collection.Label".to_string(),
        OwnedValue::from(Str::from(label)),
    )])
}

#[tokio::test]
async fn read_and_set_alias_roundtrip_after_ack() {
    let mut initial = SecretStore::new_default();
    let work_id = initial.create_collection("Work", None);
    let (service, store, script) = service_with_script(initial, vec![HostResponse::Ack]);

    let default_path = service.read_alias("default").await.unwrap();
    let work_path = collection_path(&work_id).unwrap();
    let result = service.set_alias("work", work_path).await;
    let alias_path = service.read_alias("work").await.unwrap();
    let accepted = store.read().await.clone();
    let published = script.finish();

    assert!(result.is_ok());
    assert_eq!(
        default_path.as_str(),
        format!("{BASE_PATH}/collection/default")
    );
    assert_eq!(
        alias_path.as_str(),
        format!("{BASE_PATH}/collection/{work_id}")
    );
    assert_eq!(accepted.get_alias("work"), Some(work_id.as_str()));
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].generation, 0);
    assert_eq!(published[0].section.aliases.get("work"), Some(&work_id));
}

#[tokio::test]
async fn create_collection_reject_preserves_live_state_and_registers_nothing() {
    let initial = SecretStore::new_default();
    let before = initial.clone();
    let (service, store, script) = service_with_script(initial, vec![HostResponse::Reject]);
    let object_server = ObjectServerHarness::new().await;
    let expected_path = format!("{BASE_PATH}/collection/rejected_collection");

    let result = {
        let server = object_server.object_server();
        service
            .create_collection(
                collection_properties("rejected_collection"),
                "rejected_alias",
                &server,
            )
            .await
    };
    let registered = object_server.has_collection(&expected_path).await;
    let after = store.read().await.clone();
    object_server.close().await;
    let published = script.finish();

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(after, before);
    assert!(!registered);
    assert_eq!(published[0].generation, 0);
    assert!(
        published[0]
            .section
            .collections
            .contains_key("rejected_collection")
    );
}

#[tokio::test]
async fn create_collection_ack_commits_and_registers_returned_path() {
    let initial = SecretStore::new_default();
    let (service, store, script) = service_with_script(initial, vec![HostResponse::Ack]);
    let object_server = ObjectServerHarness::new().await;
    let expected_path = format!("{BASE_PATH}/collection/accepted_collection");

    let result = {
        let server = object_server.object_server();
        service
            .create_collection(
                collection_properties("accepted_collection"),
                "accepted_alias",
                &server,
            )
            .await
    };
    let registered = object_server.has_collection(&expected_path).await;
    let accepted = store.read().await.clone();
    object_server.close().await;
    let published = script.finish();

    let (path, prompt) = result.unwrap();
    assert_eq!(path.as_str(), expected_path);
    assert_eq!(prompt.as_str(), "/");
    assert!(registered);
    assert!(accepted.collections.contains_key("accepted_collection"));
    assert_eq!(
        accepted.get_alias("accepted_alias"),
        Some("accepted_collection")
    );
    assert_eq!(published[0].generation, 0);
    assert!(
        published[0]
            .section
            .collections
            .contains_key("accepted_collection")
    );
}

#[tokio::test]
async fn set_alias_set_reject_preserves_live_state() {
    let mut initial = SecretStore::new_default();
    initial.create_collection("Work", None);
    let before = initial.clone();
    let (service, store, script) = service_with_script(initial, vec![HostResponse::Reject]);

    let result = service
        .set_alias("work", collection_path("Work").unwrap())
        .await;
    let after = store.read().await.clone();
    let published = script.finish();

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(after, before);
    assert_eq!(
        published[0].section.aliases.get("work").map(String::as_str),
        Some("Work")
    );
}

#[tokio::test]
async fn set_alias_clear_reject_preserves_live_state() {
    let mut initial = SecretStore::new_default();
    initial.set_alias("retained", Some("default"));
    let before = initial.clone();
    let (service, store, script) = service_with_script(initial, vec![HostResponse::Reject]);

    let result = service.set_alias("retained", empty_path()).await;
    let after = store.read().await.clone();
    let published = script.finish();

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(after, before);
    assert!(!published[0].section.aliases.contains_key("retained"));
}

#[tokio::test]
async fn wrong_generation_preserves_live_state() {
    let mut initial = SecretStore::new_default();
    initial.create_collection("Work", None);
    let before = initial.clone();
    let (service, store, script) =
        service_with_script(initial, vec![HostResponse::WrongGeneration]);

    let result = service
        .set_alias("work", collection_path("Work").unwrap())
        .await;
    let after = store.read().await.clone();
    let published = script.finish();

    assert!(matches!(result, Err(fdo::Error::Failed(_))));
    assert_eq!(after, before);
    assert_eq!(published[0].generation, 0);
}

#[tokio::test]
async fn rejected_mutation_then_unrelated_ack_publishes_only_later_accepted_change() {
    let initial = SecretStore::new_default();
    let before = initial.clone();
    let (service, store, script) =
        service_with_script(initial, vec![HostResponse::Reject, HostResponse::Ack]);
    let rejected = service
        .set_alias("rejected_alias", collection_path("default").unwrap())
        .await;
    let after_reject = store.read().await.clone();
    let accepted = service
        .set_alias("later_alias", collection_path("default").unwrap())
        .await;
    let final_store = store.read().await.clone();
    let published = script.finish();

    assert!(matches!(rejected, Err(fdo::Error::Failed(_))));
    assert_eq!(after_reject, before);
    assert!(accepted.is_ok());
    assert_eq!(published.len(), 2);
    assert_eq!(published[0].generation, 0);
    assert_eq!(published[1].generation, 1);
    assert_eq!(
        published[0]
            .section
            .aliases
            .get("rejected_alias")
            .map(String::as_str),
        Some("default")
    );
    assert!(!published[1].section.aliases.contains_key("rejected_alias"));
    assert_eq!(
        published[1]
            .section
            .aliases
            .get("later_alias")
            .map(String::as_str),
        Some("default")
    );
    assert_eq!(final_store.collections["default"].label, "Default");
    assert!(!final_store.aliases.contains_key("rejected_alias"));
    assert_eq!(final_store.get_alias("later_alias"), Some("default"));
}

// Mirrors the preparation order in the production handler for focused input
// validation checks that do not need a D-Bus object server or publication.
fn try_create(store: &mut SecretStore, label: &str) -> Result<String, &'static str> {
    validate_collection_name(label)?;
    Ok(store.create_collection(label, None))
}

#[test]
fn create_collection_path_is_derived_from_name_and_same_name_is_idempotent() {
    for name in ["gogcli", "awsssocli"] {
        let mut store = SecretStore::new_default();
        let first = try_create(&mut store, name).unwrap();
        let before = store.collections.len();
        let second = try_create(&mut store, name).unwrap();
        assert_eq!(first, second);
        assert_eq!(store.collections.len(), before);
        assert_eq!(
            collection_path(&first).unwrap().as_str(),
            format!("{BASE_PATH}/collection/{name}")
        );
    }
}

#[test]
fn create_collection_name_validation_accepts_clients_and_rejects_invalid_input() {
    for name in ["gogcli", "awsssocli", "login", "default", "col_1", "9to5"] {
        assert!(validate_collection_name(name).is_ok(), "{name:?}");
    }

    let too_long = "x".repeat(MAX_COLLECTION_NAME_LEN + 1);
    for bad in ["", "a/b", "naïve", "a b", "a.b", "a-b", too_long.as_str()] {
        let mut store = SecretStore::new_default();
        let before = store.clone();
        assert!(try_create(&mut store, bad).is_err(), "{bad:?}");
        assert_eq!(store, before, "{bad:?}");
    }
}

#[tokio::test]
async fn search_items_and_get_secrets_use_in_memory_values() {
    let mut initial = SecretStore::new_default();
    let attributes = HashMap::from([("service".to_string(), "synthetic".to_string())]);
    let (item_id, _) = initial
        .create_item(
            "default",
            "Synthetic",
            &attributes,
            b"synthetic-value",
            "text/plain",
            false,
        )
        .unwrap();
    let (service, _, script) = service_with_script(initial, vec![]);

    let (unlocked, locked) = service.search_items(attributes).await.unwrap();
    let item_path_string = format!("{BASE_PATH}/collection/default/{item_id}");
    let session = ObjectPath::try_from(format!("{BASE_PATH}/session/s0")).unwrap();
    let secrets = service
        .get_secrets(
            vec![ObjectPath::try_from(item_path_string.as_str()).unwrap()],
            session,
        )
        .await
        .unwrap();
    let secret = secrets
        .get(&OwnedObjectPath::try_from(item_path_string.clone()).unwrap())
        .unwrap();
    let secret_value = secret.value.clone();
    let content_type = secret.content_type.clone();
    script.finish();

    assert!(locked.is_empty());
    assert_eq!(unlocked.len(), 1);
    assert_eq!(unlocked[0].as_str(), item_path_string);
    assert_eq!(secret_value, b"synthetic-value");
    assert_eq!(content_type, "text/plain");
}
