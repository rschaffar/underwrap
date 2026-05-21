//! Runtime-client identity: map a presented mTLS client cert to a
//! known client-instance record, validate the in-memory mappings at
//! startup, and (in tests) read/write the policy document directly so
//! the runtime handlers can be exercised end-to-end.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;

use crate::runtime_state::AdminRuntimeState;
use crate::store_registry::StoreRegistry;

/// Outcome of authenticating a runtime client request. Carries the
/// authenticated instance id and the resolved store id; the live
/// backend is looked up at request time so that lock/unlock
/// transitions take effect immediately.
pub(crate) struct RuntimeRequestPrincipal {
    pub(crate) instance_id: String,
    pub(crate) store_id: String,
}

pub(crate) fn validate_runtime_client_mappings(
    stores: &Arc<std::sync::RwLock<StoreRegistry>>,
    client_instances: &Arc<
        std::sync::RwLock<HashMap<String, crate::runtime_clients::ClientInstanceRecord>>,
    >,
) -> Result<()> {
    let registry = stores.read().unwrap();
    for instance in client_instances.read().unwrap().values() {
        // Existence is all we require here: the store may be locked
        // (encrypted, no key yet) and that is still a valid mapping —
        // runtime requests against a locked store surface
        // `store_locked` at request time.
        if !registry.records.contains_key(&instance.current_store_id) {
            anyhow::bail!(
                "runtime client instance '{}' references unknown store '{}'",
                instance.id,
                instance.current_store_id
            );
        }
    }
    Ok(())
}

pub(crate) fn resolve_runtime_client(
    state: &AdminRuntimeState,
    certs: &[rustls::pki_types::CertificateDer<'static>],
) -> Result<RuntimeRequestPrincipal> {
    let cert = certs
        .first()
        .context("client did not present a certificate")?;
    let fingerprint = crate::runtime_clients::cert_fingerprint_from_der(cert.as_ref());
    let client_instances = state.client_instances.read().unwrap();
    let instance = client_instances
        .values()
        .find(|instance| instance.current_credential.cert_fingerprint == fingerprint)
        .with_context(|| format!("unknown client credential fingerprint '{fingerprint}'"))?;
    anyhow::ensure!(
        instance.status == crate::runtime_clients::ClientInstanceStatus::Active,
        "runtime client instance '{}' is not active",
        instance.id
    );
    // We deliberately don't fetch the backend Arc here — a store can
    // legitimately be `Locked` for this principal, and the caller has
    // to surface that as `store_locked`. Existence-only check.
    let store_id = instance.current_store_id.clone();
    if !state.stores.read().unwrap().records.contains_key(&store_id) {
        anyhow::bail!(
            "runtime client instance '{}' references unknown store '{}'",
            instance.id,
            store_id,
        );
    }
    Ok(RuntimeRequestPrincipal {
        instance_id: instance.id.clone(),
        store_id,
    })
}

#[cfg(test)]
pub(crate) fn get_policy_document(
    stores: &HashMap<String, Arc<dyn underwrap_store::StorageBackend>>,
    store_name: &str,
) -> Result<Option<String>> {
    use underwrap_protocol::policy::POLICY_DOCUMENT_SECRET_ID;
    let store = stores
        .get(store_name)
        .with_context(|| format!("unknown store '{store_name}'"))?;
    let document = store.read(POLICY_DOCUMENT_SECRET_ID)?;
    Ok(document.map(|bytes| String::from_utf8_lossy(bytes.as_ref()).into_owned()))
}

#[cfg(test)]
pub(crate) fn put_policy_document(
    stores: &HashMap<String, Arc<dyn underwrap_store::StorageBackend>>,
    store_name: &str,
    document_json: &str,
) -> Result<()> {
    use underwrap_protocol::policy::POLICY_DOCUMENT_SECRET_ID;
    let store = stores
        .get(store_name)
        .with_context(|| format!("unknown store '{store_name}'"))?;
    let _: underwrap_protocol::policy::PolicyDocument =
        serde_json::from_str(document_json).context("parsing policy document json")?;
    store.write(POLICY_DOCUMENT_SECRET_ID, document_json.as_bytes())
}
