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
///
/// Enrollment (backend client + principal id + resource-id map) is
/// resolved lazily on the first wrapper request that needs it and then
/// cached. There is no startup-time enroll/unenroll fork: a transient
/// backend hiccup at boot does not pin the daemon into an "unenrolled"
/// mode, and a backend restart after the daemon enrolled doesn't
/// matter because the cached state still works for new connections.
pub(crate) struct State {
    pub(crate) config: DaemonConfig,
    enrollment: std::sync::Mutex<Option<Enrolled>>,
    pub(crate) pending_approvals: PendingApprovals,
}

/// Cheap-to-clone snapshot of everything the daemon needs to be talking
/// to a live backend.
#[derive(Clone)]
pub(crate) struct Enrolled {
    pub(crate) store: Arc<dyn StorageBackend>,
    /// The backend-assigned runtime client instance id this daemon
    /// authenticates as over mTLS.
    pub(crate) client_instance: String,
    /// Per-wrapper-name → backend-resolved resource id. Populated by the
    /// register response so the wrapper-resource channel can address
    /// `resources/<id>` and the policy lookup can match by the same
    /// stable id.
    pub(crate) resource_ids: std::collections::HashMap<String, String>,
}

#[derive(Debug)]
pub(crate) enum EnrollmentError {
    /// The cert/key files configured for this instance aren't on disk
    /// yet. The operator hasn't run `underwrap-keygen` for this
    /// instance, or the keygen output landed somewhere else.
    MissingCredential,
    /// The backend rejected the handshake or `/v1/status` round-trip.
    /// Most often: backend is down, mTLS material is no longer
    /// accepted, or the operator hasn't pasted the cert PEM into the
    /// admin UI yet.
    Backend(anyhow::Error),
    /// The backend accepted us but rejected the wrapper-asset register
    /// call (typically auto-create is disabled and the wrapper's
    /// resource doesn't exist yet).
    RegisterAssets(anyhow::Error),
}

impl State {
    /// Build a fresh, unenrolled `State`. Enrollment happens lazily on
    /// the first wrapper request via [`State::ensure_enrolled`].
    pub(crate) fn new(config: DaemonConfig) -> Self {
        Self {
            config,
            enrollment: std::sync::Mutex::new(None),
            pending_approvals: PendingApprovals::new(),
        }
    }

    /// Return the URL the wrapper should send the operator to when the
    /// daemon can't enroll itself.
    pub(crate) fn enrollment_url(&self) -> String {
        build_enrollment_url(&self.config)
    }

    /// Try to populate (or return the cached) [`Enrolled`] snapshot.
    /// On failure we leave the cache empty so the next wrapper request
    /// retries — this is what makes the daemon self-heal across a
    /// backend restart or a delayed cert paste.
    pub(crate) fn ensure_enrolled(&self) -> Result<Enrolled, EnrollmentError> {
        // Fast path: somebody already enrolled.
        if let Some(e) = self.enrollment.lock().unwrap().as_ref() {
            return Ok(e.clone());
        }
        let mut guard = self.enrollment.lock().unwrap();
        if let Some(e) = guard.as_ref() {
            return Ok(e.clone());
        }
        let credential_on_disk = matches!(
            (
                self.config.backend.client_cert_file.as_ref(),
                self.config.backend.client_key_file.as_ref(),
            ),
            (Some(c), Some(k)) if c.exists() && k.exists(),
        );
        if !credential_on_disk {
            return Err(EnrollmentError::MissingCredential);
        }
        let (store, client_instance) =
            open_and_identify(&self.config).map_err(EnrollmentError::Backend)?;
        let resource_ids = register_wrapper_assets(store.as_ref(), &self.config)
            .map_err(EnrollmentError::RegisterAssets)?;
        let enrolled = Enrolled {
            store,
            client_instance,
            resource_ids,
        };
        *guard = Some(enrolled.clone());
        Ok(enrolled)
    }

    #[cfg(test)]
    pub(crate) fn new_enrolled_for_test(config: DaemonConfig, enrolled: Enrolled) -> Self {
        Self {
            config,
            enrollment: std::sync::Mutex::new(Some(enrolled)),
            pending_approvals: PendingApprovals::new(),
        }
    }
}

impl Enrolled {
    /// Resolve the backend resource id `wrapper_name` should address.
    /// Prefers the register-time cache; on cache miss falls back to
    /// reading the backend's resources document and looking up by the
    /// wrapper config's resource name.
    pub(crate) fn resource_id_for_wrapper(
        &self,
        config: &DaemonConfig,
        wrapper_name: &str,
    ) -> Option<String> {
        if let Some(id) = self.resource_ids.get(wrapper_name) {
            return Some(id.clone());
        }
        let wrapper = config.wrappers.get(wrapper_name)?;
        let material = self
            .store
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
    format!(
        "{}/client-instances",
        config.backend.url.trim_end_matches('/'),
    )
}

pub(crate) fn unenrolled_message(url: &str) -> String {
    format!(
        "this underwrap instance is not enrolled yet. open the enrollment page in your browser: {url}"
    )
}

pub async fn run(config: DaemonConfig, shutdown: Option<Arc<Notify>>) -> Result<()> {
    // Validate config at startup: every wrapper.secret must reference a declared secret.
    validate_config(&config)?;

    // The daemon enrolls lazily on the first wrapper request that
    // needs the backend, and retries on subsequent requests if the
    // attempt fails. That means a backend that is down at boot, or a
    // credential that hasn't been pasted into the admin UI yet, no
    // longer pins us into a permanent "unenrolled" state — and a
    // backend restart after a successful enrollment is invisible: the
    // cached state still works for new connections.
    //
    // We still *try* once at startup, best-effort, so the
    // auto-create-wrapper-assets register call lands as soon as
    // possible. Failure here is just an `info!` log; the lazy retry
    // path picks up the slack on the first wrapper invocation.
    let wrapper_to_gid = build_wrapper_gid_map(&config)?;
    let state = Arc::new(State::new(config.clone()));
    match state.ensure_enrolled() {
        Ok(snapshot) => info!(
            client_instance = %snapshot.client_instance,
            "enrolled with backend at startup",
        ),
        Err(EnrollmentError::MissingCredential) => info!(
            "client credential not yet on disk; daemon will retry enrollment on the next wrapper request",
        ),
        Err(EnrollmentError::Backend(e)) => warn!(
            err = %format!("{e:#}"),
            "backend not reachable at startup; daemon will retry enrollment on the next wrapper request",
        ),
        Err(EnrollmentError::RegisterAssets(e)) => warn!(
            err = %format!("{e:#}"),
            "wrapper asset registration failed at startup; daemon will retry on the next wrapper request",
        ),
    }

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
    /// invalid. `State::ensure_enrolled` surfaces this as
    /// `EnrollmentError::Backend` so the wrapper sees the enrollment
    /// URL message, and a later call retries automatically.
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

    #[test]
    fn ensure_enrolled_reports_missing_credential_when_files_absent() {
        let config = DaemonConfig {
            backend: BackendServiceConfig {
                url: "https://backend.example.invalid".into(),
                ca_file: None,
                client_cert_file: None,
                client_key_file: None,
            },
            wrappers: Default::default(),
        };
        let state = State::new(config);
        assert!(matches!(
            state.ensure_enrolled(),
            Err(EnrollmentError::MissingCredential),
        ));
        // And it stays not-enrolled — no caching of the failure.
        assert!(matches!(
            state.ensure_enrolled(),
            Err(EnrollmentError::MissingCredential),
        ));
    }

    /// The startup enroll/unenroll fork is gone: a failed enrollment
    /// attempt must NOT be cached, so the daemon self-heals as soon as
    /// the operator drops the cert files in place (or the backend
    /// comes back). We simulate that by creating the cert/key files
    /// between two `ensure_enrolled` calls — the first reports
    /// `MissingCredential`, the second progresses past that check and
    /// fails further along (because there is no real backend), but
    /// crucially the failure mode changes, proving the gate moved.
    #[test]
    fn ensure_enrolled_retries_after_credential_appears_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let ca = dir.path().join("ca.pem");
        let cert = dir.path().join("cert.pem");
        let key = dir.path().join("key.pem");
        let config = DaemonConfig {
            backend: BackendServiceConfig {
                url: "https://backend.invalid:8443".into(),
                ca_file: Some(ca.clone()),
                client_cert_file: Some(cert.clone()),
                client_key_file: Some(key.clone()),
            },
            wrappers: Default::default(),
        };
        let state = State::new(config);

        // First call: cert files don't exist yet.
        assert!(matches!(
            state.ensure_enrolled(),
            Err(EnrollmentError::MissingCredential),
        ));

        // Drop the files in place — like `underwrap-keygen` would.
        for path in [&ca, &cert, &key] {
            std::fs::File::create(path)
                .unwrap()
                .write_all(b"not-a-real-cert")
                .unwrap();
        }

        // Second call: gets past the cert-file gate and now fails at the
        // backend layer (TLS material is bogus, host is unreachable).
        // The change in failure mode is what proves the daemon retries
        // instead of staying pinned on the earlier verdict.
        match state.ensure_enrolled() {
            Err(EnrollmentError::Backend(_)) => {}
            Err(other) => panic!("expected Backend failure on retry, got {other:?}"),
            Ok(_) => panic!("expected Backend failure on retry, got Ok"),
        }
    }

    #[test]
    fn enrollment_url_points_at_client_instances_page() {
        let config = DaemonConfig {
            backend: BackendServiceConfig {
                url: "https://backend.example.invalid/".into(),
                ca_file: None,
                client_cert_file: None,
                client_key_file: None,
            },
            wrappers: Default::default(),
        };
        let state = State::new(config);
        assert_eq!(
            state.enrollment_url(),
            "https://backend.example.invalid/client-instances",
        );
    }
}
