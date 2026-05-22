use super::*;

fn populated_blob() -> ResourceBlob {
    let mut blob = ResourceBlob::empty();
    blob.env
        .insert("GH_TOKEN".into(), SecretBytes::from("ghp_test"));
    blob.env.insert("API_KEY".into(), SecretBytes::from("k"));
    blob.files.insert(
        "config/app.toml".into(),
        SecretBytes::from(b"x = 1\n".to_vec()),
    );
    blob.files
        .insert(".netrc".into(), SecretBytes::from("machine example"));

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

#[test]
fn garbage_input_rejected() {
    let err = ResourceBlob::from_canonical_bytes(b"not json").unwrap_err();
    assert!(format!("{err:#}").contains("parsing resource blob JSON"));
}
