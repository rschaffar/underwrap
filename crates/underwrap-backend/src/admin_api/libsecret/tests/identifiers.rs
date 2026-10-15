use super::*;
use underwrap_protocol::resource_blob::LibsecretSection;

const VALID_ITEM: &str = "0123456789abcdef0123456789abcdef";

struct Fixture {
    _tmp: tempfile::TempDir,
    state: Arc<ListenerState>,
    store_id: String,
    resource_id: String,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, store_id) = entity_test_state_with_store(tmp.path());
        let resource_id = create_resource_id_for_test(
            state.clone(),
            &admin_principal(),
            &store_id,
            "identifier-regression",
        );
        Self {
            _tmp: tmp,
            state,
            store_id,
            resource_id,
        }
    }

    fn collection(&self, id: Option<&str>) -> (StatusCode, serde_json::Value) {
        run_handler(handle_post_collection(
            State(self.state.clone()),
            Extension(admin_principal()),
            Path((self.store_id.clone(), self.resource_id.clone())),
            Body(AdminLibsecretCollectionCreateRequest {
                id: id.map(str::to_string),
                label: String::new(),
            }),
        ))
    }

    fn item(&self, cid: &str, id: Option<&str>) -> (StatusCode, serde_json::Value) {
        run_handler(handle_post_item(
            State(self.state.clone()),
            Extension(admin_principal()),
            Path((self.store_id.clone(), self.resource_id.clone(), cid.into())),
            ResourceBody(AdminLibsecretItemCreateRequest {
                id: id.map(str::to_string),
                label: String::new(),
                content_type: "text/plain".into(),
                attributes: BTreeMap::new(),
                secret_b64: String::new(),
            }),
        ))
    }

    fn alias(&self, name: &str, target: &str) -> (StatusCode, serde_json::Value) {
        run_handler(handle_put_alias(
            State(self.state.clone()),
            Extension(admin_principal()),
            Path((self.store_id.clone(), self.resource_id.clone(), name.into())),
            Body(AdminLibsecretAliasUpsertRequest {
                collection_id: target.into(),
            }),
        ))
    }

    fn blob(&self) -> ResourceBlob {
        read_blob(&self.state, &self.store_id, &self.resource_id).unwrap()
    }

    fn save(&self, blob: &ResourceBlob) {
        let store = admin_store(&self.state, &self.store_id).unwrap();
        save_resource_blob(store.as_ref(), &self.resource_id, blob).unwrap();
    }

    fn assert_unchanged(&self, expected: &ResourceBlob) {
        assert!(
            self.blob() == *expected,
            "complete resource must remain unchanged"
        );
    }

    fn assert_publishable(&self) {
        let publication = self.blob().libsecret.to_canonical_bytes().unwrap();
        assert!(
            LibsecretSection::from_strict_publication_bytes(&publication).is_ok(),
            "admin identifiers must be publishable after loading persisted state"
        );
    }
}

#[test]
fn default_admin_item_survives_stored_state_strict_publication() {
    let fixture = Fixture::new();
    let (status, collection) = fixture.collection(None);
    assert_eq!(status, StatusCode::OK);
    let cid = collection["id"].as_str().unwrap();
    let mut baseline = fixture.blob();
    baseline
        .env
        .insert("PRESERVED".into(), SecretBytes::from(Vec::new()));
    baseline
        .files
        .insert("config/preserved".into(), SecretBytes::from(Vec::new()));
    fixture.save(&baseline);
    assert_eq!(fixture.item(cid, Some(VALID_ITEM)).0, StatusCode::OK);
    let existing = fixture.blob().libsecret.collections[cid].items[VALID_ITEM].clone();
    let mut ids = std::collections::BTreeSet::from([VALID_ITEM.to_string()]);
    for _ in 0..3 {
        let (status, item) = fixture.item(cid, None);
        assert_eq!(status, StatusCode::OK);
        let id = item["id"].as_str().unwrap();
        assert!(is_libsecret_item_id(id));
        assert!(
            ids.insert(id.to_string()),
            "generated id must not replace an item"
        );
        fixture.assert_publishable();
    }
    let after = fixture.blob();
    assert!(after.env == baseline.env && after.files == baseline.files);
    assert!(after.libsecret.collections[cid].items[VALID_ITEM] == existing);
    assert_eq!(after.libsecret.collections[cid].items.len(), ids.len());
    assert_eq!(fixture.item(cid, Some(VALID_ITEM)).0, StatusCode::CONFLICT);
    fixture.assert_unchanged(&after);
}

#[test]
fn new_identifier_rejections_leave_complete_resource_unchanged() {
    let fixture = Fixture::new();
    assert_eq!(fixture.collection(Some("default")).0, StatusCode::OK);
    assert_eq!(fixture.item("default", Some(VALID_ITEM)).0, StatusCode::OK);
    assert_eq!(fixture.alias("default", "default").0, StatusCode::OK);
    let baseline = fixture.blob();
    for id in [
        "",
        "a.b",
        "a-b",
        ".",
        "..",
        "a/b",
        "a b",
        "é",
        &"a".repeat(129),
    ] {
        for (status, body) in [fixture.collection(Some(id)), fixture.alias(id, "default")] {
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["code"], "invalid_request");
            fixture.assert_unchanged(&baseline);
        }
    }
    for id in [
        "".into(),
        "item_1".into(),
        "a".repeat(31),
        "a".repeat(33),
        "A".repeat(32),
        "g".repeat(32),
        "_".repeat(32),
        "é".repeat(16),
    ] {
        let (status, body) = fixture.item("default", Some(&id));
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], "invalid_request");
        fixture.assert_unchanged(&baseline);
    }
    fixture.assert_publishable();
}

#[test]
fn explicit_identifier_controls_match_strict_publication() {
    let fixture = Fixture::new();
    for cid in ["0", "_", "Ab_09", &"a".repeat(128)] {
        assert_eq!(fixture.collection(Some(cid)).0, StatusCode::OK);
        assert_eq!(fixture.item(cid, Some(VALID_ITEM)).0, StatusCode::OK);
        assert_eq!(fixture.alias(cid, cid).0, StatusCode::OK);
        fixture.assert_publishable();
    }
}

#[test]
fn legacy_invalid_entries_remain_inspectable_and_deletable() {
    let fixture = Fixture::new();
    assert_eq!(fixture.collection(Some("default")).0, StatusCode::OK);
    assert_eq!(fixture.item("default", Some(VALID_ITEM)).0, StatusCode::OK);
    let baseline = fixture.blob();
    let mut legacy = baseline.clone();
    let item = legacy.libsecret.collections["default"].items[VALID_ITEM].clone();
    legacy
        .libsecret
        .collections
        .get_mut("default")
        .unwrap()
        .items
        .insert("item_1".into(), item);
    legacy.libsecret.collections.insert(
        "old.collection".into(),
        legacy.libsecret.collections["default"].clone(),
    );
    legacy
        .libsecret
        .aliases
        .insert("old-alias".into(), "old.collection".into());
    fixture.save(&legacy);

    // No new item or alias may extend an invalid collection's identifier graph.
    assert_eq!(
        fixture.item("old.collection", None).0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        fixture.alias("valid_alias", "old.collection").0,
        StatusCode::BAD_REQUEST
    );
    fixture.assert_unchanged(&legacy);
    let (status, tree) = run_handler(handle_get_tree(
        State(fixture.state.clone()),
        Path((fixture.store_id.clone(), fixture.resource_id.clone())),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tree["collections"].as_array().unwrap().len(), 2);
    let (status, _) = run_handler(handle_get_item_secret(
        State(fixture.state.clone()),
        Path((
            fixture.store_id.clone(),
            fixture.resource_id.clone(),
            "default".into(),
            "item_1".into(),
        )),
    ));
    assert_eq!(status, StatusCode::OK);

    let (status, deleted) = run_handler(handle_delete_item(
        State(fixture.state.clone()),
        Extension(admin_principal()),
        Path((
            fixture.store_id.clone(),
            fixture.resource_id.clone(),
            "default".into(),
            "item_1".into(),
        )),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(deleted["existed"], true);
    let (status, deleted) = run_handler(handle_delete_alias(
        State(fixture.state.clone()),
        Extension(admin_principal()),
        Path((
            fixture.store_id.clone(),
            fixture.resource_id.clone(),
            "old-alias".into(),
        )),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(deleted["existed"], true);
    let (status, deleted) = run_handler(handle_delete_collection(
        State(fixture.state.clone()),
        Extension(admin_principal()),
        Path((
            fixture.store_id.clone(),
            fixture.resource_id.clone(),
            "old.collection".into(),
        )),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(deleted["existed"], true);
    let repaired = fixture.blob();
    assert!(repaired.env == baseline.env && repaired.files == baseline.files);
    assert!(
        repaired.libsecret.collections["default"].items
            == baseline.libsecret.collections["default"].items
    );
    assert!(repaired.libsecret.aliases.is_empty());
    assert_eq!(repaired.libsecret.collections.len(), 1);
    fixture.assert_publishable();
}
