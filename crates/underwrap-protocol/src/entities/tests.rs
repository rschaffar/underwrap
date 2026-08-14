use super::*;

fn op(id: &str, username: &str) -> EntityCreator {
    EntityCreator::Operator {
        id: id.to_string(),
        username: username.to_string(),
    }
}

fn ci(id: &str) -> EntityCreator {
    EntityCreator::ClientInstance { id: id.to_string() }
}

#[test]
fn default_documents_are_empty() {
    let r = ResourcesDocument::default();
    assert!(r.resources.is_empty());
    let p = PairsDocument::default();
    assert!(p.pairs.is_empty());
}

#[test]
fn resources_roundtrip_through_json_with_both_creator_kinds() {
    let mut doc = ResourcesDocument::default();
    doc.insert(ResourceRecord {
        id: "res_aaaa".into(),
        name: "github/token".into(),
        created_by: op("op-1", "alice"),
        created_at: 1_700_000_000,
    })
    .unwrap();
    doc.insert(ResourceRecord {
        id: "res_bbbb".into(),
        name: "gws/state".into(),
        created_by: ci("client-7"),
        created_at: 1_700_000_500,
    })
    .unwrap();

    let json = serde_json::to_string(&doc).unwrap();
    let parsed: ResourcesDocument = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, doc);
}

#[test]
fn pairs_roundtrip_through_json() {
    let mut doc = PairsDocument::default();
    doc.insert(WrapperResourcePairRecord {
        wrapper: "gh-comment".into(),
        resource_id: "res_aaaa".into(),
        created_by: ci("client-7"),
        created_at: 42,
        last_used_at: None,
    })
    .unwrap();
    let json = serde_json::to_string(&doc).unwrap();
    let parsed: PairsDocument = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, doc);
}

#[test]
fn resource_insert_refuses_duplicate_name_or_id() {
    let mut doc = ResourcesDocument::default();
    doc.insert(ResourceRecord {
        id: "res_a".into(),
        name: "github/token".into(),
        created_by: op("op-1", "alice"),
        created_at: 1,
    })
    .unwrap();
    // Duplicate name, different id → rejected.
    let err = doc
        .insert(ResourceRecord {
            id: "res_b".into(),
            name: "github/token".into(),
            created_by: op("op-2", "bob"),
            created_at: 2,
        })
        .unwrap_err();
    assert_eq!(err, InsertError::Duplicate);
    // Duplicate id, different name → also rejected.
    let err = doc
        .insert(ResourceRecord {
            id: "res_a".into(),
            name: "github/other".into(),
            created_by: op("op-2", "bob"),
            created_at: 2,
        })
        .unwrap_err();
    assert_eq!(err, InsertError::Duplicate);
    // Provenance untouched.
    assert_eq!(doc.resources.len(), 1);
    match &doc.resources[0].created_by {
        EntityCreator::Operator { username, .. } => assert_eq!(username, "alice"),
        _ => panic!("creator changed"),
    }
    assert_eq!(doc.resources[0].created_at, 1);
}

#[test]
fn resource_rename_preserves_id_and_refuses_collisions() {
    let mut doc = ResourcesDocument::default();
    doc.insert(ResourceRecord {
        id: "res_a".into(),
        name: "github/token".into(),
        created_by: op("op-1", "alice"),
        created_at: 1,
    })
    .unwrap();
    doc.insert(ResourceRecord {
        id: "res_b".into(),
        name: "gws/state".into(),
        created_by: op("op-1", "alice"),
        created_at: 2,
    })
    .unwrap();

    // Rename to a free name succeeds; the id is unchanged.
    assert!(doc.rename("res_a", "github/main").unwrap());
    assert_eq!(doc.find_by_id("res_a").unwrap().name, "github/main");

    // Renaming an unknown id is Ok(false), not an error.
    assert!(!doc.rename("res_zzz", "anything").unwrap());

    // Renaming onto another resource's name fails.
    let err = doc.rename("res_a", "gws/state").unwrap_err();
    assert_eq!(err, InsertError::Duplicate);

    // Renaming to the same name is a no-op — allowed.
    assert!(doc.rename("res_a", "github/main").unwrap());
}

#[test]
fn pair_insert_refuses_duplicate_wrapper_resource_pair() {
    let mut doc = PairsDocument::default();
    doc.insert(WrapperResourcePairRecord {
        wrapper: "gh".into(),
        resource_id: "res_r".into(),
        created_by: ci("c1"),
        created_at: 1,
        last_used_at: None,
    })
    .unwrap();
    let err = doc
        .insert(WrapperResourcePairRecord {
            wrapper: "gh".into(),
            resource_id: "res_r".into(),
            created_by: ci("c2"),
            created_at: 2,
            last_used_at: None,
        })
        .unwrap_err();
    assert_eq!(err, InsertError::Duplicate);
    // Same wrapper, different resource — allowed.
    doc.insert(WrapperResourcePairRecord {
        wrapper: "gh".into(),
        resource_id: "res_r2".into(),
        created_by: ci("c1"),
        created_at: 3,
        last_used_at: None,
    })
    .unwrap();
    assert_eq!(doc.pairs.len(), 2);
}

#[test]
fn pairs_for_resource_filters_correctly() {
    let mut doc = PairsDocument::default();
    doc.insert(WrapperResourcePairRecord {
        wrapper: "gh".into(),
        resource_id: "res_r".into(),
        created_by: ci("c1"),
        created_at: 1,
        last_used_at: None,
    })
    .unwrap();
    doc.insert(WrapperResourcePairRecord {
        wrapper: "tf".into(),
        resource_id: "res_r".into(),
        created_by: ci("c1"),
        created_at: 2,
        last_used_at: None,
    })
    .unwrap();
    doc.insert(WrapperResourcePairRecord {
        wrapper: "tf".into(),
        resource_id: "res_other".into(),
        created_by: ci("c1"),
        created_at: 3,
        last_used_at: None,
    })
    .unwrap();
    let mut wrappers: Vec<_> = doc
        .pairs_for_resource("res_r")
        .map(|p| p.wrapper.clone())
        .collect();
    wrappers.sort();
    assert_eq!(wrappers, vec!["gh".to_string(), "tf".to_string()]);
}

#[test]
fn pair_remove_returns_whether_pair_was_removed() {
    let mut p = PairsDocument::default();
    p.insert(WrapperResourcePairRecord {
        wrapper: "gh".into(),
        resource_id: "res_r".into(),
        created_by: ci("c1"),
        created_at: 1,
        last_used_at: None,
    })
    .unwrap();
    assert!(p.remove("gh", "res_r"));
    assert!(!p.remove("gh", "res_r"));
}
