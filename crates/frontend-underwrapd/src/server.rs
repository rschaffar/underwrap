//! Unix socket server for underwrapd.
//!
//! Runs wrapper socket listeners.

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinSet;
use tracing::{error, info, warn};

use underwrap_protocol::config::DaemonConfig;
use underwrap_protocol::service::{
    ServiceWrapperRegisterReject, ServiceWrapperRegisterRequest, ServiceWrapperResourcePair,
};

use crate::approval::PendingApprovals;
use crate::startup::{bind_socket, build_wrapper_gid_map, next_listener_result, validate_config};
use crate::wrapper_flow::handle_wrapper;
use underwrap_store::StorageBackend;

/// Maximum concurrent connections per socket.
const MAX_CONNECTIONS: usize = 128;

/// Shared daemon state passed to connection handlers.
pub(crate) struct State {
    pub(crate) config: DaemonConfig,
    pub(crate) store: Option<Arc<dyn StorageBackend>>,
    pub(crate) enrollment_url: Option<String>,
    /// The backend-assigned runtime client instance id this daemon
    /// authenticates as over mTLS. Empty for unenrolled instances —
    /// the daemon refuses wrapper requests in that state anyway, so
    /// the field is only consulted on the enrolled path.
    pub(crate) client_instance: String,
    /// Per-wrapper-name → backend-resolved resource id. Populated from
    /// the register response so the wrapper-resource channel can address
    /// `resources/<id>` (and the daemon's policy lookup can match by
    /// the same stable id) without re-resolving on every read.
    pub(crate) resource_ids: std::collections::HashMap<String, String>,
    pub(crate) pending_approvals: PendingApprovals,
}

impl State {
    /// Resolve the backend resource id this `wrapper_name` should
    /// address. Prefers the register-time cache; on cache miss reads
    /// the backend's resources document and looks up by the wrapper
    /// config's resource name. Returns `None` only when neither the
    /// cache nor the catalog can resolve the wrapper (typical for
    /// unenrolled daemons, or when the resource hasn't been created
    /// on the backend yet).
    pub(crate) fn resource_id_for_wrapper(&self, wrapper_name: &str) -> Option<String> {
        if let Some(id) = self.resource_ids.get(wrapper_name) {
            return Some(id.clone());
        }
        let store = self.store.as_ref()?;
        let wrapper = self.config.wrappers.get(wrapper_name)?;
        let material = store
            .read(underwrap_protocol::entities::RESOURCES_DOCUMENT_SECRET_ID)
            .ok()
            .flatten()?;
        let doc: underwrap_protocol::entities::ResourcesDocument =
            serde_json::from_slice(material.as_ref()).ok()?;
        doc.resources
            .iter()
            .find(|r| r.name == wrapper.resource)
            .map(|r| r.id.clone())
    }
}

impl State {
    /// Get a clone of the storage backend Arc for sharing with channel handlers.
    pub(crate) fn store_arc(&self) -> Arc<dyn StorageBackend> {
        Arc::clone(
            self.store
                .as_ref()
                .expect("storage backend is only available for enrolled instances"),
        )
    }
}

/// Register all configured wrappers with the backend. Returns the
/// per-wrapper-name → resolved resource id map the backend produced,
/// or an empty map if registration failed (the daemon stays usable
/// only if the resource ids were already cached elsewhere — at
/// present we just degrade to "wrapper reads will fail").
fn register_wrapper_assets(
    store: &dyn StorageBackend,
    config: &DaemonConfig,
) -> Result<std::collections::HashMap<String, String>> {
    let mut pairs = Vec::new();
    // Track wrapper_name → resource_name on the wire so we can map
    // the response's name-keyed `resolved_resource_ids` back to the
    // wrapper key the daemon uses internally.
    let mut wrapper_to_resource_name: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for (wrapper_name, wrapper) in &config.wrappers {
        let logical_name = wrapper
            .logical_name
            .clone()
            .unwrap_or_else(|| wrapper_name.clone());
        let files_mount_paths = wrapper
            .files
            .as_ref()
            .map(|f| f.paths.clone())
            .unwrap_or_default();
        wrapper_to_resource_name.insert(wrapper_name.clone(), wrapper.resource.clone());
        pairs.push(ServiceWrapperResourcePair {
            wrapper: logical_name,
            resource: wrapper.resource.clone(),
            files_mount_paths,
        });
    }
    let response = store.register_wrapper_assets(&ServiceWrapperRegisterRequest { pairs })?;
    match response.rejected {
        None => {
            if !response.created_resources.is_empty() || !response.created_pairs.is_empty() {
                info!(
                    created_resources = response.created_resources.len(),
                    created_pairs = response.created_pairs.len(),
                    "backend auto-created wrapper assets on register",
                );
            }
        }
        Some(ServiceWrapperRegisterReject::AutoCreateDisabled {
            missing_resources,
            missing_pairs,
        }) => {
            warn!(
                missing_resources = missing_resources.len(),
                missing_pairs = missing_pairs.len(),
                "backend refused wrapper register: auto-create disabled for this client instance — \
                 an admin must enable `auto_create_wrapper_assets` on this instance, or create the \
                 missing resources and pairs manually",
            );
        }
    }
    // Project the backend's name → id map onto the daemon's
    // wrapper-name keyspace.
    let mut by_wrapper = std::collections::HashMap::new();
    for (wrapper_name, resource_name) in wrapper_to_resource_name {
        if let Some(rid) = response.resolved_resource_ids.get(&resource_name) {
            by_wrapper.insert(wrapper_name, rid.clone());
        }
    }
    Ok(by_wrapper)
}

/// Open the backend client and read the mTLS-bound client instance id
/// from `/v1/status`. Returns `Err` if either the TLS handshake or the
/// status round-trip fails — used by [`run`] to fall back to
/// unenrolled mode rather than aborting startup.
fn open_and_identify(config: &DaemonConfig) -> Result<(Arc<dyn StorageBackend>, String)> {
    let store: Arc<dyn StorageBackend> =
        Arc::from(underwrap_store::open_backend_client(&config.backend)?);
    let principal = store
        .principal()
        .context("learning own runtime client instance id from backend /v1/status")?;
    Ok((store, principal))
}

fn build_enrollment_url(config: &DaemonConfig) -> String {
    format!("{}/enroll", config.backend.url.trim_end_matches('/'),)
}

pub(crate) fn unenrolled_message(url: &str) -> String {
    format!(
        "this underwrap instance is not enrolled yet. open the enrollment page in your browser: {url}"
    )
}

pub async fn run(config: DaemonConfig, shutdown: Option<Arc<Notify>>) -> Result<()> {
    // Validate config at startup: every wrapper.secret must reference a declared secret.
    validate_config(&config)?;

    // An instance is "unenrolled" when its client credential isn't on disk
    // yet. The NixOS module always sets `client_cert_file` / `client_key_file`
    // to per-instance paths, but those files only appear after the operator
    // runs `underwrap-keygen --instance-config /etc/underwrap/instances/<inst>.yaml`.
    // Missing-file → friendly enrollment guidance; populated → open the
    // backend client normally.
    let unenrolled = match (
        config.backend.client_cert_file.as_ref(),
        config.backend.client_key_file.as_ref(),
    ) {
        (Some(cert), Some(key)) => !cert.exists() || !key.exists(),
        _ => true,
    };
    // Open the backend client when enrolled, and learn the
    // mTLS-bound client instance id by hitting `/v1/status` once at
    // startup. The id is used to look up grants in the per-store
    // policy document.
    //
    // A failure here means the cert/key are on disk but the backend
    // does not (yet) accept this client — most often because the
    // operator hasn't pasted the cert PEM into the admin UI yet. We
    // log a warning that points at the enrollment docs and fall back
    // to the same unenrolled state as when the credential files are
    // missing, so the daemon comes up and wrappers print the
    // enrollment URL on every invocation.
    let (store, client_instance, enrollment_url): (
        Option<Arc<dyn StorageBackend>>,
        String,
        Option<String>,
    ) = if unenrolled {
        (None, String::new(), Some(build_enrollment_url(&config)))
    } else {
        match open_and_identify(&config) {
            Ok((s, p)) => {
                info!(client_instance = %p, "enrolled with backend");
                (Some(s), p, None)
            }
            Err(e) => {
                warn!(
                    err = %e,
                    "backend did not accept this client's credential — the daemon is starting in \
                     unenrolled mode. Run `underwrap-keygen --instance-config <instance.yaml>` if the \
                     credential is missing, then paste the resulting PEM into the admin UI as a new \
                     client instance (see docs/getting-started.md \"Enroll the instance\"). Wrapper \
                     invocations will print the enrollment URL until this succeeds.",
                );
                (None, String::new(), Some(build_enrollment_url(&config)))
            }
        }
    };

    let wrapper_to_gid = build_wrapper_gid_map(&config)?;
    let resource_ids = if let Some(store) = store.as_ref() {
        match register_wrapper_assets(store.as_ref(), &config) {
            Ok(map) => map,
            Err(e) => {
                warn!(err = %e, "wrapper asset registration failed; continuing without backend register");
                std::collections::HashMap::new()
            }
        }
    } else {
        std::collections::HashMap::new()
    };

    let state = Arc::new(State {
        config: config.clone(),
        store,
        enrollment_url,
        client_instance,
        resource_ids,
        pending_approvals: PendingApprovals::new(),
    });

    let mut tasks = JoinSet::new();

    for (name, wrapper) in &config.wrappers {
        let gid = *wrapper_to_gid
            .get(name)
            .expect("wrapper gid must exist after startup validation");
        let listener = bind_socket(
            &wrapper.socket,
            0o660,
            &format!("wrapper '{name}'"),
            Some(gid),
        )?;
        tasks.spawn(accept_wrapper_loop(
            listener,
            Arc::clone(&state),
            name.clone(),
            gid,
        ));
    }

    info!("daemon ready");

    match shutdown {
        Some(notify) => {
            tokio::select! {
                r = next_listener_result(&mut tasks) => r,
                _ = notify.notified() => {
                    info!("shutdown signal received");
                    tasks.abort_all();
                    while tasks.join_next().await.is_some() {}
                    Ok(())
                }
            }
        }
        None => next_listener_result(&mut tasks).await,
    }
}

async fn accept_wrapper_loop(
    listener: UnixListener,
    state: Arc<State>,
    wrapper_name: String,
    expected_gid: u32,
) -> Result<()> {
    let semaphore = Arc::new(Semaphore::new(MAX_CONNECTIONS));

    loop {
        let (stream, _addr) = listener.accept().await?;
        let state = Arc::clone(&state);
        let wrapper_name = wrapper_name.clone();
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore closed");

        tokio::spawn(async move {
            if let Err(e) = handle_wrapper(stream, state, &wrapper_name, expected_gid).await {
                error!(wrapper = %wrapper_name, "connection error: {e:#}");
            }
            drop(permit);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use underwrap_protocol::config::BackendServiceConfig;

    /// `open_and_identify` must return `Err` when the TLS material is
    /// invalid, so [`run`] can demote the daemon to unenrolled mode
    /// instead of aborting startup. Symmetric with the unenrolled-on-disk
    /// path: a credential the backend rejects should behave the same as
    /// a credential that isn't there yet.
    #[test]
    fn open_and_identify_errors_on_invalid_credential() {
        let dir = tempfile::tempdir().unwrap();
        let ca = dir.path().join("ca.pem");
        let cert = dir.path().join("cert.pem");
        let key = dir.path().join("key.pem");
        for path in [&ca, &cert, &key] {
            std::fs::File::create(path)
                .unwrap()
                .write_all(b"not-a-cert")
                .unwrap();
        }
        let config = DaemonConfig {
            backend: BackendServiceConfig {
                url: "https://backend.invalid:8443".into(),
                ca_file: Some(ca),
                client_cert_file: Some(cert),
                client_key_file: Some(key),
            },
            wrappers: Default::default(),
        };
        assert!(open_and_identify(&config).is_err());
    }
}
