use super::*;
use std::collections::BTreeMap;

fn sample_section() -> LibsecretSection {
    let mut section = LibsecretSection::default();
    let mut attrs = BTreeMap::new();
    attrs.insert("server".into(), "github.com".into());

    let mut items = BTreeMap::new();
    items.insert(
        "gh-cli".into(),
        Item {
            label: "gh CLI".into(),
            content_type: "text/plain".into(),
            created: 100,
            modified: 200,
            attributes: attrs,
            secret: SecretBytes::from("ghp_secret"),
        },
    );
    section.collections.insert(
        "default".into(),
        Collection {
            label: "Default".into(),
            created: 10,
            modified: 20,
            items,
        },
    );
    section.aliases.insert("default".into(), "default".into());
    section
}

#[test]
fn round_trip_via_store_preserves_data() {
    let section = sample_section();
    let store = store_from_section(&section);
    let back = section_from_store(&store);
    assert_eq!(back, section);
}

#[test]
fn client_named_collection_survives_checkout_roundtrip() {
    // Mirrors the ns lifecycle: a wrapper checks out an empty libsecret
    // section, the target (e.g. gog) creates a name-keyed collection and
    // writes a secret, we dump the section back at teardown, and a later
    // invocation re-checks-out. The secret must come back at the same
    // name-derived collection path the client will look for.
    let mut store = store_from_section(&LibsecretSection::default());
    let col_id = store.create_collection("gogcli", None);
    assert_eq!(col_id, "gogcli");
    let (item_id, _) = store
        .create_item(
            "gogcli",
            "client-secret",
            &std::collections::HashMap::new(),
            b"refresh-token",
            "text/plain",
            false,
        )
        .unwrap();

    // Publish to a section and re-load it in a later invocation.
    let section = section_from_store(&store);
    let reloaded = store_from_section(&section);

    assert!(
        reloaded.collections.contains_key("gogcli"),
        "collection is keyed by its name across the round-trip"
    );
    assert_eq!(
        reloaded.get_secret("gogcli", &item_id),
        Some(b"refresh-token".as_slice())
    );
}

#[test]
fn reload_then_create_item_does_not_overwrite_existing() {
    // Invocation A stores item X; invocation B reloads that state and writes a
    // different item Y. Persisted and newly generated ids must remain distinct,
    // so X survives the second write.
    let mut store_a = store_from_section(&LibsecretSection::default());
    store_a.create_collection("gogcli", None);
    let mut x_attrs = std::collections::HashMap::new();
    x_attrs.insert("kind".to_string(), "client-secret".to_string());
    let (x_id, _) = store_a
        .create_item(
            "gogcli",
            "client-secret",
            &x_attrs,
            b"the-secret",
            "text/plain",
            false,
        )
        .unwrap();

    // Invocation B: fresh secretsd reloads A's backend-returned state...
    let section = section_from_store(&store_a);
    let mut store_b = store_from_section(&section);
    // ...and writes a different item (different attributes → no replace).
    let mut y_attrs = std::collections::HashMap::new();
    y_attrs.insert("kind".to_string(), "access-token".to_string());
    let (y_id, replaced) = store_b
        .create_item(
            "gogcli",
            "access-token",
            &y_attrs,
            b"a-token",
            "text/plain",
            false,
        )
        .unwrap();

    assert!(!replaced, "different attributes must not replace");
    assert_ne!(x_id, y_id, "the new item must get a distinct id");
    assert_eq!(
        store_b.get_secret("gogcli", &x_id),
        Some(b"the-secret".as_slice()),
        "the pre-existing item must survive the new write"
    );
    assert_eq!(
        store_b.get_secret("gogcli", &y_id),
        Some(b"a-token".as_slice())
    );
}

#[test]
fn store_from_empty_section_seeds_default_collection() {
    let store = store_from_section(&LibsecretSection::default());
    assert!(store.collections.contains_key("default"));
    assert_eq!(
        store.aliases.get("default").map(String::as_str),
        Some("default")
    );
}

/// Invariant: any value secretsd accepts via D-Bus (item labels, attribute
/// keys/values, content types, and arbitrary secret bytes) must project into
/// a `LibsecretSection` that canonicalises without error and round-trips
/// stably. This is the generative cousin of a fuzz target over the JCS write
/// path — it exercises control characters, embedded NULs, non-UTF-8 secret
/// bytes, large values, and JSON-significant characters.
#[test]
fn target_modifiable_values_always_canonicalise_and_round_trip() {
    use std::collections::HashMap;
    use underwrap_secretsd_store::SecretStore;

    // Adversarial string fragments for labels / attributes / content types.
    let nasty_strings: Vec<String> = vec![
        String::new(),
        " ".to_string(),
        "\t\r\n".to_string(),
        "\u{0001}\u{0002}\u{0007}".to_string(),
        "quotes\"and\\backslash".to_string(),
        "json{}[]:,significant".to_string(),
        "🔐 é 日本語 \u{1F600}".to_string(),
        "embedded\u{0000}nul".to_string(),
        "a".repeat(8 * 1024),
    ];

    // Adversarial secret payloads, including non-UTF-8 and NUL bytes.
    let nasty_secrets: Vec<Vec<u8>> = vec![
        Vec::new(),
        vec![0u8],
        vec![0xff, 0x00, 0xfe, 0x01, 0x80],
        (0u8..=255).collect(),
        b"normal-token".to_vec(),
        vec![0xc3, 0x28], // invalid UTF-8 sequence
    ];

    let mut store = SecretStore::new_default();

    let mut n = 0usize;
    for label in &nasty_strings {
        for secret in &nasty_secrets {
            // Vary the attribute key and value across the nasty set too.
            let key = &nasty_strings[n % nasty_strings.len()];
            let val = &nasty_strings[(n + 1) % nasty_strings.len()];
            let mut attrs = HashMap::new();
            attrs.insert(key.clone(), val.clone());

            let content_type = &nasty_strings[(n + 2) % nasty_strings.len()];

            store
                .create_item("default", label, &attrs, secret, content_type, false)
                .expect("create_item should accept arbitrary target-supplied values");
            n += 1;
        }
    }

    // The whole accumulated keyring must canonicalise...
    let section = section_from_store(&store);
    let bytes = section
        .to_canonical_bytes()
        .expect("section with adversarial values must canonicalise");

    // ...and round-trip stably (bytes are JCS-canonical and idempotent).
    let reparsed =
        LibsecretSection::from_canonical_bytes(&bytes).expect("canonical bytes must parse back");
    let bytes2 = reparsed
        .to_canonical_bytes()
        .expect("reparsed section must re-canonicalise");
    assert_eq!(bytes, bytes2, "canonicalisation must be idempotent");
    assert_eq!(reparsed, section, "round-trip must preserve the section");
}

#[test]
fn store_from_section_preserves_secret_bytes() {
    let section = sample_section();
    let store = store_from_section(&section);
    let secret = store
        .collections
        .get("default")
        .and_then(|c| c.items.get("gh-cli"))
        .map(|item| item.secret.as_slice());
    assert_eq!(secret, Some(b"ghp_secret".as_slice()));
}
