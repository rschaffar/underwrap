use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn attrs_match(
    item_attrs: &HashMap<String, String>,
    search_attrs: &HashMap<String, String>,
) -> bool {
    search_attrs
        .iter()
        .all(|(k, v)| item_attrs.get(k).map(|iv| iv == v).unwrap_or(false))
}
