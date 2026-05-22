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
fn store_from_empty_section_seeds_default_collection() {
    let store = store_from_section(&LibsecretSection::default());
    assert!(store.collections.contains_key("default"));
    assert_eq!(
        store.aliases.get("default").map(String::as_str),
        Some("default")
    );
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
