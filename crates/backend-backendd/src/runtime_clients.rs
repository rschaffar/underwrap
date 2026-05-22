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
    /// Unix-epoch second the credential was issued, when known.
    pub issued_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientInstanceRecord {
    /// Stable internal identifier for this runtime client instance.
    pub id: String,
    /// Editable operator-facing display name.
    pub display_name: String,
    /// Current mapped store id.
    pub current_store_id: String,
    /// Whether this runtime client instance may currently authenticate.
    pub status: ClientInstanceStatus,
    /// Last successful runtime request time, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<u64>,
    /// Exactly one current credential for this logical instance.
    pub current_credential: CurrentCredentialRecord,
    /// When set, this instance's wrappers may auto-create resource
    /// and wrapper/resource-pair records on the runtime register
    /// endpoint. Off by default; an admin must flip it on explicitly.
    pub auto_create_wrapper_assets: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientInstancesDocument {
    #[serde(default)]
    pub instances: Vec<ClientInstanceRecord>,
}

pub fn cert_fingerprint_from_der(cert_der: &[u8]) -> String {
    hex::encode(Sha256::digest(cert_der))
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
    let doc = ClientInstancesDocument { instances: records };
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
    crate::ids::is_valid_simple_id(id)
}

#[cfg(test)]
mod tests;
