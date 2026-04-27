use super::SecretStore;

impl SecretStore {
    /// Resolve an alias to a collection ID.
    pub fn get_alias(&self, alias: &str) -> Option<&str> {
        self.aliases.get(alias).map(|s| s.as_str())
    }

    /// Set or remove an alias.
    pub fn set_alias(&mut self, alias: &str, collection_id: Option<&str>) {
        match collection_id {
            Some(id) => {
                self.aliases.insert(alias.to_string(), id.to_string());
            }
            None => {
                self.aliases.remove(alias);
            }
        }
    }

    /// List all aliases for a collection.
    pub fn list_aliases_for(&self, collection_id: &str) -> Vec<&str> {
        self.aliases
            .iter()
            .filter(|(_, v)| v.as_str() == collection_id)
            .map(|(k, _)| k.as_str())
            .collect()
    }
}
