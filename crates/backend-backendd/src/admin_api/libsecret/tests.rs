use super::*;
use axum::http::StatusCode;

#[test]
fn validate_id_accepts_typical_ids() {
    for id in ["default", "item_1", "gh-cli", "a.b", "A1"] {
        validate_id("collection", id).expect(id);
    }
}

#[test]
fn validate_id_rejects_empty() {
    let err = validate_id("collection", "").unwrap_err();
    assert_eq!(err.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn validate_id_rejects_leading_underscore() {
    let err = validate_id("collection", "_meta").unwrap_err();
    assert_eq!(err.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn validate_id_rejects_slash() {
    let err = validate_id("collection", "a/b").unwrap_err();
    assert_eq!(err.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn validate_id_rejects_too_long() {
    let id = "a".repeat(MAX_ID_LEN + 1);
    let err = validate_id("collection", &id).unwrap_err();
    assert_eq!(err.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn next_auto_id_picks_first_unused() {
    let mut existing: BTreeMap<String, ()> = BTreeMap::new();
    assert_eq!(next_auto_id("item_", &existing), "item_1");
    existing.insert("item_1".into(), ());
    assert_eq!(next_auto_id("item_", &existing), "item_2");
    existing.insert("item_3".into(), ());
    // Gaps are filled: smallest free slot is item_2.
    assert_eq!(next_auto_id("item_", &existing), "item_2");
}
