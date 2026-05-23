use super::parse_item_path;
use crate::BASE_PATH;

#[test]
fn parse_item_path_accepts_valid_secret_service_paths() {
    assert_eq!(
        parse_item_path(&format!("{BASE_PATH}/collection/default/item_1")),
        Some(("default", "item_1"))
    );
    assert_eq!(parse_item_path("/invalid"), None);
    assert_eq!(
        parse_item_path(&format!("{BASE_PATH}/collection/default")),
        None
    );
}
