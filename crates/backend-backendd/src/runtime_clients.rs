//! Runtime client-instance registry: the per-frontend records that the
//! backend's mTLS layer matches incoming runtime connections against.
//!
//! Each [`ClientInstanceRecord`] anchors one logical frontend daemon to
//! exactly one [`CurrentCredentialRecord`] (the PEM-encoded client
//! certificate + its SHA-256 fingerprint). On startup the backend
//! scans the `clients_dir` directory, parses every `<id>.json` file,
//! validates uniqueness on id / display name / fingerprint, and keeps
//! the resolved map in [`crate::runtime_state::ServiceRuntimeState`]
//! for handshake lookups.
//!
//! The mTLS-presented client certificate's fingerprint
//! (`cert_fingerprint_from_der`) is the sole identity signal — there is
//! no shared secret, no token, no allowlist beyond this registry.
//!
//! On-disk layout: one file per record at
//! `<clients_dir>/<client_instance_id>.json`. The id is the file stem
//! (so it is not duplicated inside the JSON body). Writes go through a
//! per-file tempfile + atomic rename; deletes are a single unlink.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientInstanceRecord {
    /// Stable internal identifier for this runtime client instance.
    /// On disk the id is the file stem (`<id>.json`), so it is not
    /// serialized into the JSON body — the loader backfills this from
    /// the filename and `save_client_instance` writes the body
    /// without it.
    #[serde(default, skip)]
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

pub fn current_credential_from_pem(cert_pem: String) -> Result<CurrentCredentialRecord> {
    let cert_fingerprint = cert_fingerprint_from_pem(&cert_pem)?;
    Ok(CurrentCredentialRecord {
        cert_fingerprint,
        cert_pem,
    })
}

pub fn cert_fingerprint_from_pem_file(path: &Path) -> Result<String> {
    let pem = std::fs::read_to_string(path)
        .with_context(|| format!("reading client certificate PEM {}", path.display()))?;
    cert_fingerprint_from_pem(&pem)
}

/// Path of the JSON file holding `client_instance_id`'s record.
fn record_path(dir: &Path, client_instance_id: &str) -> PathBuf {
    dir.join(format!("{client_instance_id}.json"))
}

/// Parse a single `<id>.json` file. The id is taken from the file
/// stem and backfilled into the returned record; the body itself does
/// not carry it.
fn load_one(path: &Path) -> Result<ClientInstanceRecord> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading runtime client record {}", path.display()))?;
    let mut record: ClientInstanceRecord = serde_json::from_str(&text)
        .with_context(|| format!("parsing runtime client record {}", path.display()))?;
    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "runtime client record path '{}' has no valid file stem",
                path.display()
            )
        })?
        .to_string();
    anyhow::ensure!(
        is_valid_client_instance_id(&id),
        "invalid runtime client instance id '{}' derived from file {}",
        id,
        path.display()
    );
    record.id = id;
    validate_record_fields(&record, path)?;
    Ok(record)
}

fn validate_record_fields(record: &ClientInstanceRecord, path: &Path) -> Result<()> {
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
    Ok(())
}

pub fn load_client_instances(dir: &Path) -> Result<HashMap<String, ClientInstanceRecord>> {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => {
            return Err(e).with_context(|| format!("reading clients directory {}", dir.display()));
        }
    };

    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in read {
        let entry =
            entry.with_context(|| format!("iterating clients directory {}", dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("statting {}", entry.path().display()))?;
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        // Skip files without a `.json` extension; operators may leave
        // backup files (e.g. `<id>.json.bak`) without breaking the load.
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        paths.push(path);
    }
    paths.sort();

    let mut out = HashMap::new();
    let mut display_names = BTreeSet::new();
    let mut fingerprints = BTreeSet::new();
    for path in paths {
        let record = load_one(&path)?;
        let display_name_key = record.display_name.trim().to_ascii_lowercase();
        anyhow::ensure!(
            display_names.insert(display_name_key),
            "duplicate runtime client display name '{}' across files in {}",
            record.display_name,
            dir.display()
        );
        anyhow::ensure!(
            fingerprints.insert(record.current_credential.cert_fingerprint.clone()),
            "duplicate runtime client credential fingerprint '{}' across files in {}",
            record.current_credential.cert_fingerprint,
            dir.display()
        );
        let id = record.id.clone();
        anyhow::ensure!(
            out.insert(id.clone(), record).is_none(),
            "duplicate runtime client instance id '{}' across files in {}",
            id,
            dir.display()
        );
    }
    Ok(out)
}

/// Persist one client instance record to `<dir>/<id>.json` via the
/// usual tempfile + atomic rename. The on-disk body does not carry
/// the id — it is implied by the filename.
pub fn save_client_instance(dir: &Path, record: &ClientInstanceRecord) -> Result<()> {
    anyhow::ensure!(
        is_valid_client_instance_id(&record.id),
        "invalid runtime client instance id '{}'",
        record.id,
    );
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating clients directory {}", dir.display()))?;
    let json =
        serde_json::to_vec_pretty(record).context("serialising runtime client instance record")?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).with_context(|| {
        format!(
            "creating temp client instance record under {}",
            dir.display()
        )
    })?;
    use std::io::Write;
    tmp.write_all(&json)
        .context("writing runtime client instance record tempfile")?;
    tmp.as_file()
        .sync_all()
        .context("fsync runtime client instance record tempfile")?;
    let target = record_path(dir, &record.id);
    tmp.persist(&target).map_err(|e| {
        anyhow::anyhow!(
            "atomically persisting runtime client instance record to {}: {e}",
            target.display(),
        )
    })?;
    Ok(())
}

/// Remove `<dir>/<id>.json`. Missing files are tolerated so the
/// admin delete path remains idempotent under retry.
pub fn delete_client_instance(dir: &Path, client_instance_id: &str) -> Result<()> {
    let path = record_path(dir, client_instance_id);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e)
            .with_context(|| format!("removing runtime client instance record {}", path.display())),
    }
}

pub fn is_valid_client_instance_id(id: &str) -> bool {
    crate::ids::is_valid_simple_id(id)
}

#[cfg(test)]
mod tests;
