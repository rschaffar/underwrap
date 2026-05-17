use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ClientInstanceStatus {
    Active,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CurrentCredentialRecord {
    /// Fingerprint of the currently active client certificate.
    pub cert_fingerprint: String,
    /// PEM-encoded current client certificate.
    pub cert_pem: String,
    /// Unix-epoch second the credential was issued. Optional so older
    /// test fixtures can omit it cleanly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientInstanceRecord {
    /// Stable internal identifier for this runtime client instance.
    pub id: String,
    /// Editable operator-facing display name.
    pub display_name: String,
    /// Hostname the client reported during enrollment. Optional so
    /// early/CLI-created records can omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname_hint: Option<String>,
    /// Attested configured `services.underwrap.instances.<name>`.
    pub instance_name: String,
    /// Current mapped store id.
    pub current_store_id: String,
    /// Whether this runtime client instance may currently authenticate.
    pub status: ClientInstanceStatus,
    /// Last successful runtime request time, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<u64>,
    /// Exactly one current credential for this logical instance.
    pub current_credential: CurrentCredentialRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientInstancesDocument {
    #[serde(default = "default_client_instances_version")]
    pub version: u32,
    #[serde(default)]
    pub instances: Vec<ClientInstanceRecord>,
}

impl Default for ClientInstancesDocument {
    fn default() -> Self {
        Self {
            version: default_client_instances_version(),
            instances: Vec::new(),
        }
    }
}

fn default_client_instances_version() -> u32 {
    1
}

pub fn cert_fingerprint_from_der(cert_der: &[u8]) -> String {
    let digest = Sha256::digest(cert_der);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn cert_fingerprint_from_pem(cert_pem: &str) -> Result<String> {
    let mut reader = std::io::BufReader::new(cert_pem.as_bytes());
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("parsing client certificate PEM")?;
    let cert = certs
        .first()
        .context("client certificate PEM contained no certificate")?;
    Ok(cert_fingerprint_from_der(cert.as_ref()))
}

pub fn cert_fingerprint_from_pem_file(path: &Path) -> Result<String> {
    let pem = std::fs::read_to_string(path)
        .with_context(|| format!("reading client certificate PEM {}", path.display()))?;
    cert_fingerprint_from_pem(&pem)
}

pub fn load_client_instances(path: &Path) -> Result<HashMap<String, ClientInstanceRecord>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let doc: ClientInstancesDocument = serde_json::from_str(&text)
        .with_context(|| format!("parsing runtime client registry {}", path.display()))?;
    anyhow::ensure!(
        doc.version == default_client_instances_version(),
        "unsupported runtime client registry version {}",
        doc.version
    );

    let mut out = HashMap::new();
    let mut display_names = BTreeSet::new();
    let mut fingerprints = BTreeSet::new();
    for record in doc.instances {
        anyhow::ensure!(
            is_valid_client_instance_id(&record.id),
            "invalid runtime client instance id '{}' in {}",
            record.id,
            path.display()
        );
        anyhow::ensure!(
            !record.display_name.trim().is_empty(),
            "runtime client instance '{}' has an empty display name in {}",
            record.id,
            path.display()
        );
        anyhow::ensure!(
            !record.instance_name.trim().is_empty(),
            "runtime client instance '{}' has an empty instance_name in {}",
            record.id,
            path.display()
        );
        anyhow::ensure!(
            !record.current_store_id.trim().is_empty(),
            "runtime client instance '{}' has an empty current_store_id in {}",
            record.id,
            path.display()
        );
        anyhow::ensure!(
            !record.current_credential.cert_fingerprint.trim().is_empty(),
            "runtime client instance '{}' has an empty current credential fingerprint in {}",
            record.id,
            path.display()
        );
        anyhow::ensure!(
            !record.current_credential.cert_pem.trim().is_empty(),
            "runtime client instance '{}' has an empty current credential PEM in {}",
            record.id,
            path.display()
        );

        let display_name_key = record.display_name.trim().to_ascii_lowercase();
        anyhow::ensure!(
            display_names.insert(display_name_key),
            "duplicate runtime client display name '{}' in {}",
            record.display_name,
            path.display()
        );
        anyhow::ensure!(
            fingerprints.insert(record.current_credential.cert_fingerprint.clone()),
            "duplicate runtime client credential fingerprint '{}' in {}",
            record.current_credential.cert_fingerprint,
            path.display()
        );
        let id = record.id.clone();
        anyhow::ensure!(
            out.insert(id.clone(), record).is_none(),
            "duplicate runtime client instance id '{}' in {}",
            id,
            path.display()
        );
    }
    Ok(out)
}

pub fn save_client_instances(
    path: &Path,
    instances: &HashMap<String, ClientInstanceRecord>,
) -> Result<()> {
    let mut records: Vec<ClientInstanceRecord> = instances.values().cloned().collect();
    records.sort_by(|a, b| a.id.cmp(&b.id));
    let doc = ClientInstancesDocument {
        version: default_client_instances_version(),
        instances: records,
    };
    let json = serde_json::to_vec_pretty(&doc).context("serialising runtime client registry")?;
    let dir = path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "runtime client registry path '{}' has no parent directory",
            path.display()
        )
    })?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating runtime client registry dir {}", dir.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).with_context(|| {
        format!(
            "creating temp runtime client registry under {}",
            dir.display()
        )
    })?;
    use std::io::Write;
    tmp.write_all(&json)
        .context("writing runtime client registry tempfile")?;
    tmp.as_file()
        .sync_all()
        .context("fsync runtime client registry tempfile")?;
    tmp.persist(path).map_err(|e| {
        anyhow::anyhow!(
            "atomically persisting runtime client registry to {}: {e}",
            path.display(),
        )
    })?;
    Ok(())
}

pub fn is_valid_client_instance_id(id: &str) -> bool {
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

    fn sample(id: &str) -> ClientInstanceRecord {
        ClientInstanceRecord {
            id: id.to_string(),
            display_name: format!("Display {id}"),
            hostname_hint: Some("deepthought".into()),
            instance_name: "work".into(),
            current_store_id: "store_work".into(),
            status: ClientInstanceStatus::Active,
            last_seen_at: Some(123),
            current_credential: CurrentCredentialRecord {
                cert_fingerprint: format!("fp-{id}"),
                cert_pem: "-----BEGIN CERTIFICATE-----\nZm9v\n-----END CERTIFICATE-----".into(),
                issued_at: Some(100),
            },
        }
    }

    #[test]
    fn missing_registry_file_is_tolerated() {
        let tmp = tempfile::TempDir::new().unwrap();
        let loaded = load_client_instances(&tmp.path().join("client-instances.json")).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn save_and_load_round_trip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("client-instances.json");
        let mut map = HashMap::new();
        map.insert("inst_work".into(), sample("inst_work"));
        save_client_instances(&path, &map).unwrap();
        let loaded = load_client_instances(&path).unwrap();
        assert_eq!(loaded, map);
    }

    #[test]
    fn invalid_version_is_rejected() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("client-instances.json");
        std::fs::write(&path, r#"{"version":99,"instances":[]}"#).unwrap();
        let err = load_client_instances(&path).expect_err("unsupported version must fail");
        assert!(format!("{err:#}").contains("unsupported runtime client registry version"));
    }

    #[test]
    fn duplicate_id_is_rejected() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("client-instances.json");
        let a = sample("inst_a");
        let mut b = sample("inst_b");
        b.id = a.id.clone();
        let doc = ClientInstancesDocument {
            version: 1,
            instances: vec![a, b],
        };
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        let err = load_client_instances(&path).expect_err("duplicate ids must fail");
        assert!(format!("{err:#}").contains("duplicate runtime client instance id"));
    }

    #[test]
    fn duplicate_fingerprint_is_rejected() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("client-instances.json");
        let a = sample("inst_a");
        let mut b = sample("inst_b");
        b.current_credential.cert_fingerprint = a.current_credential.cert_fingerprint.clone();
        let doc = ClientInstancesDocument {
            version: 1,
            instances: vec![a, b],
        };
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        let err = load_client_instances(&path).expect_err("duplicate fingerprints must fail");
        assert!(format!("{err:#}").contains("duplicate runtime client credential fingerprint"));
    }

    #[test]
    fn empty_required_fields_are_rejected() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("client-instances.json");
        let mut rec = sample("inst_a");
        rec.instance_name.clear();
        let doc = ClientInstancesDocument {
            version: 1,
            instances: vec![rec],
        };
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        let err = load_client_instances(&path).expect_err("empty required fields must fail");
        assert!(format!("{err:#}").contains("empty instance_name"));
    }
}
