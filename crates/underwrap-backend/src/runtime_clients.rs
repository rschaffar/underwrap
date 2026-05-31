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
use std::collections::HashMap;
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

impl crate::storage::json_dir_store::DirRecord for ClientInstanceRecord {
    const KIND: &'static str = "runtime client instance";
    const KIND_PLURAL_DIR: &'static str = "clients directory";

    fn id(&self) -> &str {
        &self.id
    }

    fn set_id(&mut self, id: String) {
        self.id = id;
    }

    fn validate_fields(&self, path: &Path) -> Result<()> {
        anyhow::ensure!(
            !self.display_name.trim().is_empty(),
            "runtime client instance '{}' has an empty display name in {}",
            self.id,
            path.display(),
        );
        anyhow::ensure!(
            !self.current_store_id.trim().is_empty(),
            "runtime client instance '{}' has an empty current_store_id in {}",
            self.id,
            path.display(),
        );
        anyhow::ensure!(
            !self.current_credential.cert_fingerprint.trim().is_empty(),
            "runtime client instance '{}' has an empty current credential fingerprint in {}",
            self.id,
            path.display(),
        );
        anyhow::ensure!(
            !self.current_credential.cert_pem.trim().is_empty(),
            "runtime client instance '{}' has an empty current credential PEM in {}",
            self.id,
            path.display(),
        );
        let derived_fingerprint = cert_fingerprint_from_pem(&self.current_credential.cert_pem)
            .with_context(|| {
                format!(
                    "parsing current credential PEM for runtime client instance '{}' in {}",
                    self.id,
                    path.display(),
                )
            })?;
        anyhow::ensure!(
            derived_fingerprint == self.current_credential.cert_fingerprint,
            "runtime client instance '{}' has a current credential fingerprint/PEM mismatch in {}",
            self.id,
            path.display(),
        );
        Ok(())
    }

    fn uniqueness_keys(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "display name",
                self.display_name.trim().to_ascii_lowercase(),
            ),
            (
                "credential fingerprint",
                self.current_credential.cert_fingerprint.clone(),
            ),
        ]
    }
}

pub fn load_client_instances(dir: &Path) -> Result<HashMap<String, ClientInstanceRecord>> {
    let records: Vec<ClientInstanceRecord> = crate::storage::json_dir_store::load_dir(dir)?;
    Ok(records.into_iter().map(|r| (r.id.clone(), r)).collect())
}

/// Persist one client instance record to `<dir>/<id>.json` via the
/// usual tempfile + atomic rename.
pub fn save_client_instance(dir: &Path, record: &ClientInstanceRecord) -> Result<()> {
    crate::storage::json_dir_store::save(dir, record)
}

/// Remove `<dir>/<id>.json`. Missing files are tolerated so the
/// admin delete path remains idempotent under retry.
pub fn delete_client_instance(dir: &Path, client_instance_id: &str) -> Result<()> {
    crate::storage::json_dir_store::delete(dir, "runtime client instance", client_instance_id)
}

#[cfg(test)]
mod tests;
