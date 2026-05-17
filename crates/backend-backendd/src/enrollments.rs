use anyhow::{Context, Result};
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnrollmentInviteRecord {
    /// Stable invite identifier. This is safe to put in URLs and commands.
    pub id: String,
    /// SHA-256 hex digest of the one-time enrollment secret. The raw secret is
    /// returned once at invite creation and is never persisted.
    pub secret_hash: String,
    /// Operator-facing name to assign to the runtime client instance.
    pub display_name: String,
    /// Hostname hint from the unenrolled instance URL, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname_hint: Option<String>,
    /// Configured `services.underwrap.instances.<name>` this invite enrolls.
    pub instance_name: String,
    /// Backend-managed store id the enrolled instance should use.
    pub current_store_id: String,
    pub created_at: u64,
    pub expires_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_client_instance_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnrollmentInvitesDocument {
    #[serde(default = "default_enrollment_invites_version")]
    pub version: u32,
    #[serde(default)]
    pub invites: Vec<EnrollmentInviteRecord>,
}

impl Default for EnrollmentInvitesDocument {
    fn default() -> Self {
        Self {
            version: default_enrollment_invites_version(),
            invites: Vec::new(),
        }
    }
}

fn default_enrollment_invites_version() -> u32 {
    1
}

pub fn generate_invite_id() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("enr_{}", hex::encode(bytes))
}

pub fn generate_enrollment_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn enrollment_secret_hash(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    hex::encode(digest)
}

pub fn verify_enrollment_secret(secret: &str, expected_hash: &str) -> bool {
    use subtle::ConstantTimeEq;
    let actual = enrollment_secret_hash(secret);
    actual.len() == expected_hash.len()
        && bool::from(actual.as_bytes().ct_eq(expected_hash.as_bytes()))
}

pub fn load_enrollment_invites(path: &Path) -> Result<HashMap<String, EnrollmentInviteRecord>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let doc: EnrollmentInvitesDocument = serde_json::from_str(&text)
        .with_context(|| format!("parsing enrollment invite registry {}", path.display()))?;
    anyhow::ensure!(
        doc.version == default_enrollment_invites_version(),
        "unsupported enrollment invite registry version {}",
        doc.version
    );

    let mut out = HashMap::new();
    let mut ids = BTreeSet::new();
    for record in doc.invites {
        anyhow::ensure!(
            is_valid_enrollment_invite_id(&record.id),
            "invalid enrollment invite id '{}' in {}",
            record.id,
            path.display()
        );
        anyhow::ensure!(
            !record.secret_hash.trim().is_empty(),
            "enrollment invite '{}' has an empty secret hash in {}",
            record.id,
            path.display()
        );
        anyhow::ensure!(
            !record.display_name.trim().is_empty(),
            "enrollment invite '{}' has an empty display name in {}",
            record.id,
            path.display()
        );
        anyhow::ensure!(
            !record.instance_name.trim().is_empty(),
            "enrollment invite '{}' has an empty instance_name in {}",
            record.id,
            path.display()
        );
        anyhow::ensure!(
            !record.current_store_id.trim().is_empty(),
            "enrollment invite '{}' has an empty current_store_id in {}",
            record.id,
            path.display()
        );
        anyhow::ensure!(
            record.expires_at > record.created_at,
            "enrollment invite '{}' expires before it is created in {}",
            record.id,
            path.display()
        );
        anyhow::ensure!(
            ids.insert(record.id.clone()),
            "duplicate enrollment invite id '{}' in {}",
            record.id,
            path.display()
        );
        out.insert(record.id.clone(), record);
    }
    Ok(out)
}

pub fn save_enrollment_invites(
    path: &Path,
    invites: &HashMap<String, EnrollmentInviteRecord>,
) -> Result<()> {
    let mut records: Vec<EnrollmentInviteRecord> = invites.values().cloned().collect();
    records.sort_by(|a, b| a.id.cmp(&b.id));
    let doc = EnrollmentInvitesDocument {
        version: default_enrollment_invites_version(),
        invites: records,
    };
    let json = serde_json::to_vec_pretty(&doc).context("serialising enrollment invite registry")?;
    let dir = path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "enrollment invite registry path '{}' has no parent directory",
            path.display()
        )
    })?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating enrollment invite registry dir {}", dir.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).with_context(|| {
        format!(
            "creating temp enrollment invite registry under {}",
            dir.display()
        )
    })?;
    use std::io::Write;
    tmp.write_all(&json)
        .context("writing enrollment invite registry tempfile")?;
    tmp.as_file()
        .sync_all()
        .context("fsync enrollment invite registry tempfile")?;
    tmp.persist(path).map_err(|e| {
        anyhow::anyhow!(
            "atomically persisting enrollment invite registry to {}: {e}",
            path.display(),
        )
    })?;
    Ok(())
}

pub fn is_valid_enrollment_invite_id(id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> EnrollmentInviteRecord {
        EnrollmentInviteRecord {
            id: id.to_string(),
            secret_hash: enrollment_secret_hash("secret"),
            display_name: "Laptop Work".into(),
            hostname_hint: Some("deepthought".into()),
            instance_name: "work".into(),
            current_store_id: "store_work".into(),
            created_at: 100,
            expires_at: 200,
            consumed_at: None,
            consumed_client_instance_id: None,
        }
    }

    #[test]
    fn missing_registry_file_is_tolerated() {
        let tmp = tempfile::TempDir::new().unwrap();
        let loaded = load_enrollment_invites(&tmp.path().join("invites.json")).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn save_and_load_round_trip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("invites.json");
        let mut map = HashMap::new();
        map.insert("enr_a".into(), sample("enr_a"));
        save_enrollment_invites(&path, &map).unwrap();
        let loaded = load_enrollment_invites(&path).unwrap();
        assert_eq!(loaded, map);
    }

    #[test]
    fn rejects_duplicate_ids() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("invites.json");
        let doc = EnrollmentInvitesDocument {
            version: 1,
            invites: vec![sample("enr_a"), sample("enr_a")],
        };
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        let err = load_enrollment_invites(&path).expect_err("duplicate ids must fail");
        assert!(format!("{err:#}").contains("duplicate enrollment invite id"));
    }

    #[test]
    fn verifies_hashed_secret_in_constant_time_path() {
        let hash = enrollment_secret_hash("correct horse battery staple");
        assert!(verify_enrollment_secret(
            "correct horse battery staple",
            &hash
        ));
        assert!(!verify_enrollment_secret("wrong", &hash));
    }
}
