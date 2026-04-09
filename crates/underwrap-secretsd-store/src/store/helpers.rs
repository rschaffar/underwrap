use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn default_content_type() -> String {
    "text/plain".to_string()
}

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Serde helper for base64-encoding Vec<u8> in JSON.
pub(crate) mod base64_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        encoded.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        use base64::Engine;
        let encoded = String::deserialize(d)?;
        base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .map_err(serde::de::Error::custom)
    }
}

pub(crate) fn attrs_match(
    item_attrs: &HashMap<String, String>,
    search_attrs: &HashMap<String, String>,
) -> bool {
    search_attrs
        .iter()
        .all(|(k, v)| item_attrs.get(k).map(|iv| iv == v).unwrap_or(false))
}
