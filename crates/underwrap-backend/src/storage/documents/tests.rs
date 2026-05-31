use super::*;
use crate::runtime_api::principal::{get_policy_document, put_policy_document};
use crate::test_support::TestStore;
use std::collections::HashMap;
use std::sync::Arc;

#[test]
fn policy_document_roundtrips_through_management_helpers() {
    let stores = HashMap::from([(
        "work".to_string(),
        Arc::new(TestStore::new()) as Arc<dyn StorageBackend>,
    )]);

    let json = r#"{"wrappers":[{"wrapper":"gh-comment","mode":{"kind":"unlocked"}}],"rules":[{"id":"r1","wrapper":"gh-comment","resource":"secret:github/token","operation":"secret.read","effect":"allow"}],"grants":[]}"#;
    put_policy_document(&stores, "work", json).unwrap();
    let stored = get_policy_document(&stores, "work").unwrap();
    assert_eq!(stored.as_deref(), Some(json));
}

#[test]
fn resources_document_roundtrips_through_load_save() {
    use underwrap_protocol::entities::{EntityCreator, ResourceRecord, ResourcesDocument};
    let store = TestStore::new();
    assert_eq!(
        load_resources_document(&store).unwrap(),
        ResourcesDocument::default(),
    );
    let mut doc = ResourcesDocument::default();
    doc.insert(ResourceRecord {
        id: "res_token".into(),
        name: "github/token".into(),
        created_by: EntityCreator::Operator {
            id: "op-1".into(),
            username: "alice".into(),
        },
        created_at: 1_700_000_000,
    })
    .unwrap();
    save_resources_document(&store, &doc).unwrap();
    assert_eq!(load_resources_document(&store).unwrap(), doc);
}

#[test]
fn pairs_document_roundtrips_through_load_save() {
    use underwrap_protocol::entities::{EntityCreator, PairsDocument, WrapperResourcePairRecord};
    let store = TestStore::new();
    assert_eq!(
        load_pairs_document(&store).unwrap(),
        PairsDocument::default(),
    );
    let mut doc = PairsDocument::default();
    doc.insert(WrapperResourcePairRecord {
        wrapper: "gh-comment".into(),
        resource_id: "res_token".into(),
        created_by: EntityCreator::ClientInstance {
            id: "client-7".into(),
        },
        created_at: 42,
        last_used_at: None,
    })
    .unwrap();
    save_pairs_document(&store, &doc).unwrap();
    assert_eq!(load_pairs_document(&store).unwrap(), doc);
}

#[test]
fn put_policy_document_rejects_invalid_json() {
    let stores = HashMap::from([(
        "work".to_string(),
        Arc::new(TestStore::new()) as Arc<dyn StorageBackend>,
    )]);

    let err = put_policy_document(&stores, "work", "not-json").unwrap_err();
    assert!(err.to_string().contains("parsing policy document json"));
}
