use std::collections::HashMap;

use super::SecretStore;
use super::helpers::attrs_match;

impl SecretStore {
    /// Search items in a collection by attribute match.
    pub fn search(&self, collection: &str, attributes: &HashMap<String, String>) -> Vec<&str> {
        self.collections
            .get(collection)
            .map(|col| {
                col.items
                    .iter()
                    .filter(|(_, item)| attrs_match(&item.attributes, attributes))
                    .map(|(id, _)| id.as_str())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Search all collections. Returns `(collection_id, [item_ids])` pairs.
    pub fn search_all(&self, attributes: &HashMap<String, String>) -> Vec<(&str, Vec<&str>)> {
        self.collections
            .iter()
            .filter_map(|(col_id, col)| {
                let matches: Vec<&str> = col
                    .items
                    .iter()
                    .filter(|(_, item)| attrs_match(&item.attributes, attributes))
                    .map(|(id, _)| id.as_str())
                    .collect();
                if matches.is_empty() {
                    None
                } else {
                    Some((col_id.as_str(), matches))
                }
            })
            .collect()
    }
}
