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
    anyhow::ensure!(
        certs.len() == 1,
        "client certificate PEM must contain exactly one certificate"
    );
    Ok(cert_fingerprint_from_der(cert.as_ref()))
}

pub fn current_credential_from_pem(
    cert_pem: String,
    issued_at: Option<u64>,
) -> Result<CurrentCredentialRecord> {
    let cert_fingerprint = cert_fingerprint_from_pem(&cert_pem)?;
    Ok(CurrentCredentialRecord {
        cert_fingerprint,
        cert_pem,
        issued_at,
    })
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
        let derived_fingerprint = cert_fingerprint_from_pem(&record.current_credential.cert_pem)
            .with_context(|| {
                format!(
                    "parsing current credential PEM for runtime client instance '{}' in {}",
                    record.id,
                    path.display()
                )
            })?;
        anyhow::ensure!(
            derived_fingerprint == record.current_credential.cert_fingerprint,
            "runtime client instance '{}' has a current credential fingerprint/PEM mismatch in {}",
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

    fn sample_cert_pem(label: &str) -> String {
        let key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(vec![label.to_string()]).unwrap();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, label);
        params.not_before = rcgen::date_time_ymd(2025, 1, 1);
        params.not_after = rcgen::date_time_ymd(2035, 1, 1);
        params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ClientAuth];
        params.serial_number = Some(rcgen::SerialNumber::from(1));
        let cert = params.self_signed(&key).unwrap();
        cert.pem()
    }

    fn sample(id: &str) -> ClientInstanceRecord {
        ClientInstanceRecord {
            id: id.to_string(),
            display_name: format!("Display {id}"),
            instance_name: "work".into(),
            current_store_id: "store_work".into(),
            status: ClientInstanceStatus::Active,
            last_seen_at: Some(123),
            current_credential: current_credential_from_pem(sample_cert_pem(id), Some(100))
                .unwrap(),
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
        b.current_credential.cert_pem = a.current_credential.cert_pem.clone();
        let doc = ClientInstancesDocument {
            version: 1,
            instances: vec![a, b],
        };
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        let err = load_client_instances(&path).expect_err("duplicate fingerprints must fail");
        assert!(format!("{err:#}").contains("duplicate runtime client credential fingerprint"));
    }

    #[test]
    fn mismatched_current_credential_fingerprint_is_rejected() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("client-instances.json");
        let mut rec = sample("inst_a");
        rec.current_credential.cert_fingerprint = "wrong".into();
        let doc = ClientInstancesDocument {
            version: 1,
            instances: vec![rec],
        };
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        let err = load_client_instances(&path).expect_err("mismatched fingerprints must fail");
        assert!(format!("{err:#}").contains("fingerprint/PEM mismatch"));
    }

    #[test]
    fn multiple_certificates_in_current_credential_are_rejected() {
        let err = cert_fingerprint_from_pem(&format!(
            "{}{}",
            sample_cert_pem("first"),
            sample_cert_pem("second")
        ))
        .expect_err("multiple certs must be rejected");
        assert!(format!("{err:#}").contains("exactly one certificate"));
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
