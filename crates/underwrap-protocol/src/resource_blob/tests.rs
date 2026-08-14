use super::*;

fn blob_with_exact_canonical_len(len: usize) -> ResourceBlob {
    let mut blob = ResourceBlob::empty();
    let base = blob.to_canonical_bytes().unwrap().len();
    // Item labels are ordinary JCS strings, so ASCII padding gives exact byte
    // control without relying on an unrelated raw-byte vector.
    let padding = len
        .checked_sub(base + 52)
        .expect("test length below fixture overhead");
    blob.libsecret.collections.insert(
        "c".to_string(),
        Collection {
            label: "x".repeat(padding),
            created: 0,
            modified: 0,
            items: BTreeMap::new(),
        },
    );
    assert_eq!(serde_jcs::to_vec(&blob).unwrap().len(), len);
    blob
}

#[test]
fn complete_resource_size_boundary_is_inclusive_and_checked_before_parse() {
    for len in [
        MAX_CANONICAL_RESOURCE_BYTES - 1,
        MAX_CANONICAL_RESOURCE_BYTES,
    ] {
        let blob = blob_with_exact_canonical_len(len);
        let bytes = blob.to_canonical_bytes().unwrap();
        assert_eq!(ResourceBlob::from_canonical_bytes(&bytes).unwrap(), blob);
    }
    let oversized = blob_with_exact_canonical_len(MAX_CANONICAL_RESOURCE_BYTES + 1);
    let bytes = serde_jcs::to_vec(&oversized).unwrap();
    let error = ResourceBlob::from_canonical_bytes(&bytes).unwrap_err();
    assert!(is_resource_too_large_error(&error));
    let size = resource_too_large_error(&error).expect("typed resource-size error");
    assert_eq!(size.attempted_len(), MAX_CANONICAL_RESOURCE_BYTES + 1);
    assert_eq!(size.maximum_len(), MAX_CANONICAL_RESOURCE_BYTES);
    assert_eq!(
        error.to_string(),
        format!(
            "resource blob exceeds maximum of {MAX_CANONICAL_RESOURCE_BYTES} bytes (got {})",
            MAX_CANONICAL_RESOURCE_BYTES + 1
        )
    );

    let serialization_error = oversized.to_canonical_bytes().unwrap_err();
    assert!(is_resource_too_large_error(&serialization_error));
    assert_eq!(
        resource_too_large_error(&serialization_error)
            .expect("typed serialization-size error")
            .attempted_len(),
        MAX_CANONICAL_RESOURCE_BYTES + 1
    );
}

fn populated_blob() -> ResourceBlob {
    let mut blob = ResourceBlob::empty();
    blob.env
        .insert("GH_TOKEN".into(), SecretBytes::from("ghp_test"));
    blob.env.insert("API_KEY".into(), SecretBytes::from("k"));
    blob.files.insert(
        ".config/gh/hosts.yml".into(),
        SecretBytes::from(b"github.com:\n  user: alice\n".to_vec()),
    );
    blob.files.insert(
        ".config/git/config".into(),
        SecretBytes::from(b"[user]\n  email = a@b\n".to_vec()),
    );

    let mut items = BTreeMap::new();
    let mut attrs = BTreeMap::new();
    attrs.insert("server".into(), "github.com".into());
    attrs.insert("user".into(), "alice".into());
    items.insert(
        "gh-cli".into(),
        Item {
            label: "gh CLI token".into(),
            content_type: "text/plain".into(),
            created: 1_000,
            modified: 2_000,
            attributes: attrs,
            secret: SecretBytes::from("ghp_secret"),
        },
    );

    let mut collections = BTreeMap::new();
    collections.insert(
        "default".into(),
        Collection {
            label: "Default".into(),
            created: 100,
            modified: 200,
            items,
        },
    );

    let mut aliases = BTreeMap::new();
    aliases.insert("default".into(), "default".into());

    blob.libsecret = LibsecretSection {
        aliases,
        collections,
    };
    blob
}

#[test]
fn content_references_are_complete_identifier_only_and_deterministically_ordered() {
    let mut blob = populated_blob();
    blob.env.insert(
        "AAA_SECRET_NAME".into(),
        SecretBytes::from("env-value-must-not-copy"),
    );
    blob.files.insert(
        "aaa/private".into(),
        SecretBytes::from("file-value-must-not-copy"),
    );
    // Enumeration is representational, not validation: this dangling target
    // still contributes exactly one alias reference.
    blob.libsecret
        .aliases
        .insert("aaa-dangling".into(), "target-must-not-copy".into());
    blob.libsecret.collections.insert(
        "aaa-empty".into(),
        Collection {
            label: "collection-label-must-not-copy".into(),
            created: 91,
            modified: 92,
            items: BTreeMap::new(),
        },
    );

    let references = blob.content_references();
    assert_eq!(
        references,
        vec![
            ResourceContentReference::Env {
                name: "AAA_SECRET_NAME".into(),
            },
            ResourceContentReference::Env {
                name: "API_KEY".into(),
            },
            ResourceContentReference::Env {
                name: "GH_TOKEN".into(),
            },
            ResourceContentReference::File {
                path: ".config/gh/hosts.yml".into(),
            },
            ResourceContentReference::File {
                path: ".config/git/config".into(),
            },
            ResourceContentReference::File {
                path: "aaa/private".into(),
            },
            ResourceContentReference::LibsecretAlias {
                name: "aaa-dangling".into(),
            },
            ResourceContentReference::LibsecretAlias {
                name: "default".into(),
            },
            ResourceContentReference::LibsecretCollection {
                collection_id: "aaa-empty".into(),
            },
            ResourceContentReference::LibsecretCollection {
                collection_id: "default".into(),
            },
            ResourceContentReference::LibsecretItem {
                collection_id: "default".into(),
                item_id: "gh-cli".into(),
            },
        ]
    );
    assert_eq!(references.len(), 11);

    let json = serde_json::to_value(&references).unwrap();
    assert_eq!(
        json[0],
        serde_json::json!({"kind": "env", "name": "AAA_SECRET_NAME"})
    );
    assert_eq!(
        json[8],
        serde_json::json!({
            "kind": "libsecret_collection",
            "collection_id": "aaa-empty"
        })
    );
    assert_eq!(
        json[10],
        serde_json::json!({
            "kind": "libsecret_item",
            "collection_id": "default",
            "item_id": "gh-cli"
        })
    );
    let serialized = serde_json::to_string(&json).unwrap();
    for excluded in [
        "env-value-must-not-copy",
        "file-value-must-not-copy",
        "target-must-not-copy",
        "collection-label-must-not-copy",
        "gh CLI token",
        "github.com",
        "alice",
        "text/plain",
        "ghp_secret",
        "1000",
        "2000",
    ] {
        assert!(
            !serialized.contains(excluded),
            "value-bearing field leaked into references: {excluded}"
        );
    }
}

#[test]
fn empty_blob_has_no_content_references() {
    assert!(ResourceBlob::empty().content_references().is_empty());
}

#[test]
fn libsecret_section_round_trips_independently() {
    let blob = populated_blob();
    let section = blob.libsecret.clone();
    let bytes = section.to_canonical_bytes().unwrap();
    let decoded = LibsecretSection::from_canonical_bytes(&bytes).unwrap();
    assert_eq!(decoded, section);
    // Byte-stable on re-encode.
    assert_eq!(decoded.to_canonical_bytes().unwrap(), bytes);
}

#[test]
fn empty_libsecret_section_round_trips() {
    let section = LibsecretSection::default();
    let bytes = section.to_canonical_bytes().unwrap();
    let decoded = LibsecretSection::from_canonical_bytes(&bytes).unwrap();
    assert_eq!(decoded, section);
}

#[test]
fn empty_blob_round_trips() {
    let blob = ResourceBlob::empty();
    let bytes = blob.to_canonical_bytes().unwrap();
    let decoded = ResourceBlob::from_canonical_bytes(&bytes).unwrap();
    assert_eq!(decoded, blob);
}

#[test]
fn populated_blob_round_trips() {
    let blob = populated_blob();
    let bytes = blob.to_canonical_bytes().unwrap();
    let decoded = ResourceBlob::from_canonical_bytes(&bytes).unwrap();
    assert_eq!(decoded, blob);
}

#[test]
fn canonicalization_is_byte_stable_across_round_trip() {
    let blob = populated_blob();
    let first = blob.to_canonical_bytes().unwrap();
    let decoded = ResourceBlob::from_canonical_bytes(&first).unwrap();
    let second = decoded.to_canonical_bytes().unwrap();
    assert_eq!(first, second);
}

#[test]
fn equal_blobs_built_independently_have_equal_bytes() {
    let a = populated_blob();
    let b = populated_blob();
    assert_eq!(
        a.to_canonical_bytes().unwrap(),
        b.to_canonical_bytes().unwrap()
    );
}

#[test]
fn unicode_strings_round_trip_byte_stable() {
    let mut blob = ResourceBlob::empty();
    blob.env
        .insert("GREETING".into(), SecretBytes::from("héllo 🌍 ñ"));
    let mut collection_items = BTreeMap::new();
    let mut attrs = BTreeMap::new();
    attrs.insert("café".into(), "naïve".into());
    attrs.insert("kanji".into(), "日本語".into());
    collection_items.insert(
        "üñîçødé".into(),
        Item {
            label: "Ω test".into(),
            content_type: "text/plain".into(),
            created: 1,
            modified: 2,
            attributes: attrs,
            secret: SecretBytes::from("seçret"),
        },
    );
    let mut cols = BTreeMap::new();
    cols.insert(
        "default".into(),
        Collection {
            label: "Déflt".into(),
            created: 0,
            modified: 0,
            items: collection_items,
        },
    );
    blob.libsecret.collections = cols;

    let first = blob.to_canonical_bytes().unwrap();
    let decoded = ResourceBlob::from_canonical_bytes(&first).unwrap();
    assert_eq!(decoded, blob);
    let second = decoded.to_canonical_bytes().unwrap();
    assert_eq!(first, second);
}

/// Timestamps near the ES6 safe-integer ceiling (2^53 − 1) round
/// trip cleanly. Anything above that would be silently converted
/// to floating point by JCS and fail to deserialize back into
/// `u64` — but no realistic Unix-seconds timestamp comes anywhere
/// near 2^53, which is ~285M years past the epoch.
#[test]
fn large_safe_u64_round_trips() {
    // 2^53 - 1, the largest integer ES6 represents exactly.
    let big: u64 = (1u64 << 53) - 1;
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
fn unknown_schema_version_rejected() {
    let blob = ResourceBlob {
        schema_version: 999,
        ..ResourceBlob::empty()
    };
    // We hand-encode via serde_jcs so the version makes it onto the wire.
    let bytes = serde_jcs::to_vec(&blob).unwrap();
    let err = ResourceBlob::from_canonical_bytes(&bytes).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("unsupported resource blob schema_version 999"),
        "unexpected error: {msg}"
    );
}

#[test]
fn maps_serialize_in_key_sorted_order() {
    // Build two blobs with the same keys inserted in different
    // orders. BTreeMap doesn't expose insertion order — this test
    // is here as a defense against the schema accidentally being
    // changed to use HashMap. If someone ever does, this still
    // can't catch it directly; but the byte-stability tests
    // above will fail randomly on CI, which is the real signal.
    // Here we just assert that the resulting bytes contain keys
    // in sorted order.
    let mut blob = ResourceBlob::empty();
    blob.env.insert("BBB".into(), SecretBytes::from("b"));
    blob.env.insert("AAA".into(), SecretBytes::from("a"));
    blob.env.insert("CCC".into(), SecretBytes::from("c"));

    let bytes = blob.to_canonical_bytes().unwrap();
    let json = std::str::from_utf8(&bytes).unwrap();
    let aaa = json.find("\"AAA\"").unwrap();
    let bbb = json.find("\"BBB\"").unwrap();
    let ccc = json.find("\"CCC\"").unwrap();
    assert!(aaa < bbb && bbb < ccc, "keys not sorted in {json}");
}

#[test]
fn empty_sections_omit_no_data_but_round_trip() {
    let bytes = ResourceBlob::empty().to_canonical_bytes().unwrap();
    let decoded = ResourceBlob::from_canonical_bytes(&bytes).unwrap();
    assert!(decoded.env.is_empty());
    assert!(decoded.files.is_empty());
    assert!(decoded.libsecret.aliases.is_empty());
    assert!(decoded.libsecret.collections.is_empty());
}

#[test]
fn raw_byte_secrets_round_trip() {
    // Non-UTF-8 bytes in a secret must survive (base64 encoding).
    let mut blob = ResourceBlob::empty();
    blob.env
        .insert("BIN".into(), SecretBytes::from(vec![0u8, 1, 2, 255, 128]));
    blob.files
        .insert("blob".into(), SecretBytes::from(vec![0xff, 0xfe, 0xfd]));

    let bytes = blob.to_canonical_bytes().unwrap();
    let decoded = ResourceBlob::from_canonical_bytes(&bytes).unwrap();
    assert_eq!(decoded, blob);
}

fn strict_publication_fixture() -> LibsecretSection {
    let mut section = LibsecretSection::default();
    section.collections.insert(
        "default".into(),
        Collection {
            label: "arbitrary label / unicode ✓".into(),
            created: 1,
            modified: 2,
            items: BTreeMap::from([(
                "0123456789abcdef0123456789abcdef".into(),
                Item {
                    label: "item".into(),
                    content_type: "application/octet-stream".into(),
                    created: 3,
                    modified: 4,
                    attributes: BTreeMap::from([("arbitrary/key".into(), "value".into())]),
                    secret: SecretBytes::from(vec![0, 255]),
                },
            )]),
        },
    );
    section.aliases.insert("default".into(), "default".into());
    section
}

#[test]
fn libsecret_strict_publication_accepts_exact_canonical_safe_graph() {
    let section = strict_publication_fixture();
    let bytes = section.to_canonical_bytes().unwrap();
    assert_eq!(
        LibsecretSection::from_strict_publication_bytes(&bytes).unwrap(),
        section
    );
}

#[test]
fn libsecret_strict_publication_rejects_independent_noncanonical_forms() {
    let canonical = strict_publication_fixture().to_canonical_bytes().unwrap();
    let text = std::str::from_utf8(&canonical).unwrap();

    let whitespace = format!(" {text}\n");
    assert!(LibsecretSection::from_strict_publication_bytes(whitespace.as_bytes()).is_err());

    let inner = &text[1..text.len() - 1];
    let aliases_end = inner.find(",\"collections\":").unwrap();
    let aliases = &inner[..aliases_end];
    let collections = &inner[aliases_end + 1..];
    let reordered = format!("{{{collections},{aliases}}}");
    assert_eq!(
        LibsecretSection::from_canonical_bytes(reordered.as_bytes()).unwrap(),
        strict_publication_fixture()
    );
    assert!(LibsecretSection::from_strict_publication_bytes(reordered.as_bytes()).is_err());

    let escaped = text.replace("unicode ✓", "unicode \\u2713");
    assert_ne!(escaped, text);
    assert_eq!(
        LibsecretSection::from_canonical_bytes(escaped.as_bytes()).unwrap(),
        strict_publication_fixture()
    );
    assert!(LibsecretSection::from_strict_publication_bytes(escaped.as_bytes()).is_err());

    assert!(
        LibsecretSection::from_strict_publication_bytes(
            br#"{"aliases":{},"collections":{},"unknown":1}"#
        )
        .is_err()
    );
    assert!(LibsecretSection::from_strict_publication_bytes(br#"{"collections":{}}"#).is_err());
    assert!(
        LibsecretSection::from_strict_publication_bytes(
            br#"{"aliases":{},"aliases":{},"collections":{}}"#
        )
        .is_err()
    );
}

#[test]
fn libsecret_strict_publication_validates_segments_items_aliases_and_references() {
    for bad in ["", "bad/slash", "bad-dash", "naïve"] {
        let mut section = strict_publication_fixture();
        let collection = section.collections.remove("default").unwrap();
        section.collections.insert(bad.into(), collection);
        section.aliases.clear();
        let bytes = section.to_canonical_bytes().unwrap();
        assert!(
            LibsecretSection::from_strict_publication_bytes(&bytes).is_err(),
            "{bad:?}"
        );
    }
    let mut long = strict_publication_fixture();
    let collection = long.collections.remove("default").unwrap();
    long.collections.insert("x".repeat(129), collection);
    long.aliases.clear();
    assert!(
        LibsecretSection::from_strict_publication_bytes(&long.to_canonical_bytes().unwrap())
            .is_err()
    );

    for bad in [
        "0123456789abcdef0123456789abcde",
        "0123456789abcdef0123456789abcdef0",
        "0123456789abcdef0123456789abcdeF",
        "0123456789abcdef0123456789abcdeg",
        "0123456789abcdef/123456789abcdef",
    ] {
        let mut section = strict_publication_fixture();
        let collection = section.collections.get_mut("default").unwrap();
        let item = collection.items.pop_first().unwrap().1;
        collection.items.insert(bad.into(), item);
        assert!(
            LibsecretSection::from_strict_publication_bytes(&section.to_canonical_bytes().unwrap())
                .is_err(),
            "{bad:?}"
        );
    }

    for bad_alias in ["", "bad/alias", "bad-alias"] {
        let mut section = strict_publication_fixture();
        section.aliases.clear();
        section.aliases.insert(bad_alias.into(), "default".into());
        assert!(
            LibsecretSection::from_strict_publication_bytes(&section.to_canonical_bytes().unwrap())
                .is_err()
        );
    }
    let mut long_alias = strict_publication_fixture();
    long_alias.aliases.clear();
    long_alias.aliases.insert("a".repeat(129), "default".into());
    assert!(
        LibsecretSection::from_strict_publication_bytes(&long_alias.to_canonical_bytes().unwrap())
            .is_err()
    );

    let mut exact_boundaries = strict_publication_fixture();
    let collection = exact_boundaries.collections.remove("default").unwrap();
    let collection_id = "c".repeat(128);
    exact_boundaries
        .collections
        .insert(collection_id.clone(), collection);
    exact_boundaries.aliases.clear();
    exact_boundaries
        .aliases
        .insert("a".repeat(128), collection_id);
    let bytes = exact_boundaries.to_canonical_bytes().unwrap();
    assert_eq!(
        LibsecretSection::from_strict_publication_bytes(&bytes).unwrap(),
        exact_boundaries
    );

    let mut dangling = strict_publication_fixture();
    dangling.aliases.insert("other".into(), "missing".into());
    assert!(
        LibsecretSection::from_strict_publication_bytes(&dangling.to_canonical_bytes().unwrap())
            .is_err()
    );
}

#[test]
fn libsecret_strict_publication_accepts_exact_cap_minus_one_and_cap_bodies() {
    for wanted in [
        crate::libsecret_publication::MAX_BODY_BYTES - 1,
        crate::libsecret_publication::MAX_BODY_BYTES,
    ] {
        let mut section = strict_publication_fixture();
        section
            .collections
            .get_mut("default")
            .unwrap()
            .label
            .clear();
        let base_len = section.to_canonical_bytes().unwrap().len();
        section.collections.get_mut("default").unwrap().label = "x".repeat(wanted - base_len);
        let bytes = section.to_canonical_bytes().unwrap();
        assert_eq!(bytes.len(), wanted);
        assert_eq!(
            LibsecretSection::from_strict_publication_bytes(&bytes).unwrap(),
            section
        );
    }
}

#[test]
fn libsecret_strict_publication_enforces_safe_timestamp_without_temporal_rules() {
    let max = (1_u64 << 53) - 1;
    let mut section = strict_publication_fixture();
    let collection = section.collections.get_mut("default").unwrap();
    collection.created = max;
    collection.modified = 0;
    let item = collection.items.values_mut().next().unwrap();
    item.created = max;
    item.modified = 0;
    let bytes = section.to_canonical_bytes().unwrap();
    LibsecretSection::from_strict_publication_bytes(&bytes).unwrap();

    section.collections.get_mut("default").unwrap().created = max + 1;
    let bytes = section.to_canonical_bytes().unwrap();
    assert!(LibsecretSection::from_strict_publication_bytes(&bytes).is_err());
}

#[test]
fn garbage_input_rejected() {
    let err = ResourceBlob::from_canonical_bytes(b"not json").unwrap_err();
    assert!(format!("{err:#}").contains("parsing resource blob JSON"));
}
