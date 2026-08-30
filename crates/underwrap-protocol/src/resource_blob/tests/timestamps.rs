use super::*;

const EXPECTED_TIMESTAMP_ERROR: &str = "libsecret timestamp exceeds 2^53 - 1";
const ITEM_ID: &str = "0123456789abcdef0123456789abcdef";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimestampField {
    CollectionCreated,
    CollectionModified,
    ItemCreated,
    ItemModified,
}

impl TimestampField {
    const ALL: [Self; 4] = [
        Self::CollectionCreated,
        Self::CollectionModified,
        Self::ItemCreated,
        Self::ItemModified,
    ];

    fn get(self, section: &LibsecretSection) -> u64 {
        let collection = section.collections.get("default").unwrap();
        let item = collection.items.get(ITEM_ID).unwrap();
        match self {
            Self::CollectionCreated => collection.created,
            Self::CollectionModified => collection.modified,
            Self::ItemCreated => item.created,
            Self::ItemModified => item.modified,
        }
    }

    fn set(self, section: &mut LibsecretSection, value: u64) {
        let collection = section.collections.get_mut("default").unwrap();
        match self {
            Self::CollectionCreated => collection.created = value,
            Self::CollectionModified => collection.modified = value,
            Self::ItemCreated => collection.items.get_mut(ITEM_ID).unwrap().created = value,
            Self::ItemModified => collection.items.get_mut(ITEM_ID).unwrap().modified = value,
        }
    }

    fn values(self, selected: u64) -> (u64, u64, u64, u64) {
        let mut values = (11, 12, 13, 14);
        match self {
            Self::CollectionCreated => values.0 = selected,
            Self::CollectionModified => values.1 = selected,
            Self::ItemCreated => values.2 = selected,
            Self::ItemModified => values.3 = selected,
        }
        values
    }
}

#[derive(Clone, Copy, Debug)]
enum CanonicalContainer {
    ResourceBlob,
    LibsecretSection,
}

impl CanonicalContainer {
    const ALL: [Self; 2] = [Self::ResourceBlob, Self::LibsecretSection];
}

fn timestamp_fixture(field: TimestampField, value: u64) -> LibsecretSection {
    let mut section = strict_publication_fixture();
    field.set(&mut section, value);
    section
}

fn encode_container(
    container: CanonicalContainer,
    section: LibsecretSection,
) -> anyhow::Result<Vec<u8>> {
    match container {
        CanonicalContainer::ResourceBlob => {
            let mut blob = ResourceBlob::empty();
            blob.libsecret = section;
            blob.to_canonical_bytes()
        }
        CanonicalContainer::LibsecretSection => section.to_canonical_bytes(),
    }
}

fn decode_container(
    container: CanonicalContainer,
    bytes: &[u8],
) -> anyhow::Result<LibsecretSection> {
    match container {
        CanonicalContainer::ResourceBlob => {
            Ok(ResourceBlob::from_canonical_bytes(bytes)?.libsecret)
        }
        CanonicalContainer::LibsecretSection => LibsecretSection::from_canonical_bytes(bytes),
    }
}

fn raw_section_json(field: TimestampField, value: u64, collection_id: &str, label: &str) -> String {
    let (collection_created, collection_modified, item_created, item_modified) =
        field.values(value);
    r#"{"aliases":{"default":"$collection_id"},"collections":{"$collection_id":{"created":$collection_created,"items":{"$item_id":{"attributes":{},"content_type":"text/plain","created":$item_created,"label":"item","modified":$item_modified,"secret":"cw=="}},"label":"$label","modified":$collection_modified}}}"#
        .replace("$collection_created", &collection_created.to_string())
        .replace("$collection_modified", &collection_modified.to_string())
        .replace("$item_created", &item_created.to_string())
        .replace("$item_modified", &item_modified.to_string())
        .replace("$collection_id", collection_id)
        .replace("$item_id", ITEM_ID)
        .replace("$label", label)
}

fn raw_resource_json(section: &str, schema_version: u32) -> String {
    format!(
        r#"{{"env":{{}},"files":{{}},"libsecret":{section},"schema_version":{schema_version}}}"#
    )
}

fn raw_container_json(container: CanonicalContainer, field: TimestampField, value: u64) -> Vec<u8> {
    let section = raw_section_json(field, value, "default", "collection");
    match container {
        CanonicalContainer::ResourceBlob => raw_resource_json(&section, CURRENT_SCHEMA_VERSION),
        CanonicalContainer::LibsecretSection => section,
    }
    .into_bytes()
}

fn assert_other_timestamps_safe(section: &LibsecretSection, selected: TimestampField) {
    for field in TimestampField::ALL {
        if field != selected {
            assert!(field.get(section) < MAX_LIBSECRET_TIMESTAMP);
        }
    }
}

fn assert_timestamp_error(error: &anyhow::Error) {
    let chain = format!("{error:#}");
    assert!(
        chain.contains(EXPECTED_TIMESTAMP_ERROR),
        "unexpected error chain: {chain}"
    );
    assert!(
        !chain.contains(&(MAX_LIBSECRET_TIMESTAMP + 1).to_string()),
        "timestamp error disclosed the rejected value: {chain}"
    );
}

/// Timestamps near the ES6 safe-integer ceiling (2^53 − 1) round trip cleanly.
#[test]
fn large_safe_u64_round_trips() {
    let big = MAX_LIBSECRET_TIMESTAMP;
    let mut blob = ResourceBlob::empty();
    let mut items = BTreeMap::new();
    items.insert(
        "huge".into(),
        Item {
            label: "huge".into(),
            content_type: "text/plain".into(),
            created: big - 1,
            modified: big,
            attributes: BTreeMap::new(),
            secret: SecretBytes::from("s"),
        },
    );
    let mut cols = BTreeMap::new();
    cols.insert(
        "default".into(),
        Collection {
            label: "d".into(),
            created: big - 2,
            modified: big - 1,
            items,
        },
    );
    blob.libsecret.collections = cols;

    let first = blob.to_canonical_bytes().unwrap();
    let decoded = ResourceBlob::from_canonical_bytes(&first).unwrap();
    assert_eq!(decoded, blob);
    let second = decoded.to_canonical_bytes().unwrap();
    assert_eq!(first, second);
}

#[test]
fn four_fields_and_two_containers_enforce_exact_encode_decode_boundary_matrix() {
    let mut outcomes = 0;
    for field in TimestampField::ALL {
        for container in CanonicalContainer::ALL {
            let maximum = timestamp_fixture(field, MAX_LIBSECRET_TIMESTAMP);
            assert_other_timestamps_safe(&maximum, field);
            encode_container(container, maximum).unwrap();
            outcomes += 1;

            let raw_maximum = raw_container_json(container, field, MAX_LIBSECRET_TIMESTAMP);
            let decoded = decode_container(container, &raw_maximum).unwrap();
            assert_eq!(field.get(&decoded), MAX_LIBSECRET_TIMESTAMP);
            assert_other_timestamps_safe(&decoded, field);
            outcomes += 1;

            let above_maximum = timestamp_fixture(field, MAX_LIBSECRET_TIMESTAMP + 1);
            assert_other_timestamps_safe(&above_maximum, field);
            let error = encode_container(container, above_maximum).unwrap_err();
            assert_timestamp_error(&error);
            outcomes += 1;

            let raw_above_maximum =
                raw_container_json(container, field, MAX_LIBSECRET_TIMESTAMP + 1);
            let error = decode_container(container, &raw_above_maximum).unwrap_err();
            assert_timestamp_error(&error);
            outcomes += 1;
        }
    }
    assert_eq!(outcomes, 32);
}

#[test]
fn direct_publication_semantics_enforce_all_fields_without_temporal_rules() {
    let mut all_maximum = strict_publication_fixture();
    for field in TimestampField::ALL {
        field.set(&mut all_maximum, MAX_LIBSECRET_TIMESTAMP);
    }
    for field in TimestampField::ALL {
        assert_eq!(field.get(&all_maximum), MAX_LIBSECRET_TIMESTAMP);
    }
    all_maximum.validate_publication_semantics().unwrap();

    let mut created_after_modified = strict_publication_fixture();
    created_after_modified
        .collections
        .get_mut("default")
        .unwrap()
        .created = MAX_LIBSECRET_TIMESTAMP;
    created_after_modified
        .collections
        .get_mut("default")
        .unwrap()
        .modified = 0;
    created_after_modified
        .collections
        .get_mut("default")
        .unwrap()
        .items
        .get_mut(ITEM_ID)
        .unwrap()
        .created = MAX_LIBSECRET_TIMESTAMP;
    created_after_modified
        .collections
        .get_mut("default")
        .unwrap()
        .items
        .get_mut(ITEM_ID)
        .unwrap()
        .modified = 0;
    created_after_modified
        .validate_publication_semantics()
        .unwrap();

    for field in TimestampField::ALL {
        let section = timestamp_fixture(field, MAX_LIBSECRET_TIMESTAMP + 1);
        let error = section.validate_publication_semantics().unwrap_err();
        assert_eq!(error.to_string(), EXPECTED_TIMESTAMP_ERROR);
        assert_timestamp_error(&error);
    }
}

#[test]
fn libsecret_strict_publication_enforces_safe_timestamp_without_temporal_rules() {
    let mut maximum = strict_publication_fixture();
    for field in TimestampField::ALL {
        field.set(&mut maximum, MAX_LIBSECRET_TIMESTAMP);
    }
    let bytes = maximum.to_canonical_bytes().unwrap();
    assert_eq!(
        LibsecretSection::from_strict_publication_bytes(&bytes).unwrap(),
        maximum
    );

    for field in TimestampField::ALL {
        let typed = timestamp_fixture(field, MAX_LIBSECRET_TIMESTAMP + 1);
        let error = typed.to_canonical_bytes().unwrap_err();
        assert_timestamp_error(&error);

        let raw = raw_container_json(
            CanonicalContainer::LibsecretSection,
            field,
            MAX_LIBSECRET_TIMESTAMP + 1,
        );
        let error = LibsecretSection::from_strict_publication_bytes(&raw).unwrap_err();
        assert_timestamp_error(&error);
    }
}

#[test]
fn size_checks_and_timestamp_validation_preserve_mixed_defect_precedence() {
    let large_label = "x".repeat(MAX_CANONICAL_RESOURCE_BYTES);
    let oversized_section = raw_section_json(
        TimestampField::CollectionCreated,
        MAX_LIBSECRET_TIMESTAMP + 1,
        "default",
        &large_label,
    );
    let oversized_resource = raw_resource_json(&oversized_section, CURRENT_SCHEMA_VERSION);
    assert!(oversized_resource.len() > MAX_CANONICAL_RESOURCE_BYTES);
    let error = ResourceBlob::from_canonical_bytes(oversized_resource.as_bytes()).unwrap_err();
    assert!(is_resource_too_large_error(&error));

    let publication_label = "x".repeat(crate::libsecret_publication::MAX_BODY_BYTES);
    let oversized_publication = raw_section_json(
        TimestampField::CollectionCreated,
        MAX_LIBSECRET_TIMESTAMP + 1,
        "default",
        &publication_label,
    );
    assert!(oversized_publication.len() > crate::libsecret_publication::MAX_BODY_BYTES);
    let error = LibsecretSection::from_strict_publication_bytes(oversized_publication.as_bytes())
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "libsecret publication exceeds the 1 MiB cap"
    );

    let unsafe_section = raw_section_json(
        TimestampField::CollectionCreated,
        MAX_LIBSECRET_TIMESTAMP + 1,
        "default",
        "collection",
    );
    let unknown_schema = raw_resource_json(&unsafe_section, 999);
    let error = ResourceBlob::from_canonical_bytes(unknown_schema.as_bytes()).unwrap_err();
    assert_timestamp_error(&error);
    assert!(!format!("{error:#}").contains("unsupported resource blob schema_version"));

    let noncanonical = format!(" {unsafe_section}\n");
    let error =
        LibsecretSection::from_strict_publication_bytes(noncanonical.as_bytes()).unwrap_err();
    assert_timestamp_error(&error);
    assert!(!format!("{error:#}").contains("not exact JCS canonical bytes"));

    let semantic_defect = raw_section_json(
        TimestampField::CollectionCreated,
        MAX_LIBSECRET_TIMESTAMP + 1,
        "bad-id",
        "collection",
    );
    let error =
        LibsecretSection::from_strict_publication_bytes(semantic_defect.as_bytes()).unwrap_err();
    assert_timestamp_error(&error);
    assert!(!format!("{error:#}").contains("invalid collection id path segment"));

    let mut eventual_oversize = timestamp_fixture(
        TimestampField::CollectionCreated,
        MAX_LIBSECRET_TIMESTAMP + 1,
    );
    eventual_oversize
        .collections
        .get_mut("default")
        .unwrap()
        .label = large_label;
    let mut safe_oversize = eventual_oversize.clone();
    TimestampField::CollectionCreated.set(&mut safe_oversize, MAX_LIBSECRET_TIMESTAMP);
    let mut safe_blob = ResourceBlob::empty();
    safe_blob.libsecret = safe_oversize;
    let error = safe_blob.to_canonical_bytes().unwrap_err();
    assert!(is_resource_too_large_error(&error));

    let mut unsafe_blob = ResourceBlob::empty();
    unsafe_blob.libsecret = eventual_oversize;
    let error = unsafe_blob.to_canonical_bytes().unwrap_err();
    assert_timestamp_error(&error);
    assert!(!is_resource_too_large_error(&error));
}
