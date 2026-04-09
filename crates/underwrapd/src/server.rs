//! Unix socket server for underwrapd.
//!
//! Runs wrapper socket listeners plus the control socket listener.

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinSet;
use tokio::time::{Duration, timeout};
use tracing::{error, info, warn};

#[cfg(test)]
use underwrap_protocol::config::WrapperDef;
use underwrap_protocol::config::{AccessMode, DaemonConfig, UnlockMode};
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::*;

use crate::audit;
use crate::control::handle_control;
use crate::peer_auth::get_peer_cred;
use crate::startup::{
    bind_socket, build_allowed_user_uids, build_wrapper_gid_map, next_listener_result,
    validate_config,
};
use crate::unlock::UnlockState;
use crate::wrapper_bindings::{
    wrapper_directory_bindings, wrapper_env_credentials, wrapper_libsecret_binding,
};
use ssh_key::public::KeyData;
use underwrap_store::StorageBackend;

/// Maximum concurrent connections per socket.
const MAX_CONNECTIONS: usize = 128;

/// Timeout for the first frame on a wrapper socket.
const WRAPPER_FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(2);

/// Timeout for the unlock challenge follow-up frame on the same connection.
const WRAPPER_UNLOCK_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

/// Shared daemon state passed to connection handlers.
pub(crate) struct State {
    pub(crate) config: DaemonConfig,
    pub(crate) store: Arc<dyn StorageBackend>,
    pub(crate) allowed_user_uids: std::collections::HashSet<u32>,
    pub(crate) unlock: UnlockState,
}

impl State {
    /// Get a clone of the storage backend Arc for sharing with channel handlers.
    fn store_arc(&self) -> Arc<dyn StorageBackend> {
        Arc::clone(&self.store)
    }
}

/// Run the daemon until `shutdown` is notified (or forever if `None`).
pub async fn run(config: DaemonConfig, shutdown: Option<Arc<Notify>>) -> Result<()> {
    // Validate config at startup: every wrapper.secret must reference a declared secret.
    validate_config(&config)?;

    // Open the storage backend.
    let store: Arc<dyn StorageBackend> = match &config.storage {
        underwrap_protocol::config::StorageConfig::Remote { socket } => {
            Arc::new(underwrap_store::RemoteBackend::open(socket)?)
        }
        other => {
            let boxed = underwrap_store::open(other)?;
            // Convert Box<dyn StorageBackend> to Arc<dyn StorageBackend>.
            Arc::from(boxed)
        }
    };

    let wrapper_to_gid = build_wrapper_gid_map(&config)?;
    let allowed_user_uids = build_allowed_user_uids(&config)?;
    let state = Arc::new(State {
        config: config.clone(),
        store,
        allowed_user_uids,
        unlock: UnlockState::new(),
    });

    let control_listener = bind_socket(
        &config.sockets.control,
        config.sockets.control_mode,
        "control",
        None,
    )?;

    let mut tasks = JoinSet::new();
    tasks.spawn(accept_loop(
        control_listener,
        Arc::clone(&state),
        handle_control,
    ));

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

async fn accept_loop<F, Fut>(listener: UnixListener, state: Arc<State>, handler: F) -> Result<()>
where
    F: Fn(UnixStream, Arc<State>) -> Fut + Send + Sync + Copy + 'static,
    Fut: std::future::Future<Output = Result<()>> + Send,
{
    let semaphore = Arc::new(Semaphore::new(MAX_CONNECTIONS));

    loop {
        let (stream, _addr) = listener.accept().await?;
        let state = Arc::clone(&state);
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore closed");

        tokio::spawn(async move {
            if let Err(e) = handler(stream, state).await {
                error!("connection error: {e:#}");
            }
            drop(permit);
        });
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

// ---------------------------------------------------------------------------
// Wrapper socket handler
// ---------------------------------------------------------------------------

async fn handle_wrapper(
    mut stream: UnixStream,
    state: Arc<State>,
    wrapper_name: &str,
    expected_gid: u32,
) -> Result<()> {
    let peer = get_peer_cred(&stream);
    let peer_uid = match peer.as_ref().map(|p| p.uid) {
        Some(uid) => uid,
        None => {
            let response = WrapperResponse::Error {
                code: ErrorCode::Unauthorized,
                message: "could not determine peer uid".to_string(),
            };
            write_frame_async(&mut stream, &response).await?;
            stream.shutdown().await?;
            return Ok(());
        }
    };

    if !state.allowed_user_uids.is_empty() && !state.allowed_user_uids.contains(&peer_uid) {
        let reason = format!("peer uid {peer_uid} is not allowed for this instance");
        audit::auth_denied(
            "wrapper",
            &reason,
            Some(peer_uid),
            peer.as_ref().map(|p| p.gid),
        );
        let response = WrapperResponse::Error {
            code: ErrorCode::Unauthorized,
            message: reason,
        };
        write_frame_async(&mut stream, &response).await?;
        stream.shutdown().await?;
        return Ok(());
    }

    if peer.as_ref().and_then(|p| p.egid) != Some(expected_gid) {
        let reason = format!(
            "peer egid {:?} does not match wrapper '{}' gid {}",
            peer.as_ref().and_then(|p| p.egid),
            wrapper_name,
            expected_gid,
        );
        warn!(wrapper = %wrapper_name, peer = ?peer, "wrapper identity rejected");
        audit::auth_denied(
            "wrapper",
            &reason,
            Some(peer_uid),
            peer.as_ref().map(|p| p.gid),
        );
        let response = WrapperResponse::Error {
            code: ErrorCode::Unauthorized,
            message: reason,
        };
        write_frame_async(&mut stream, &response).await?;
        stream.shutdown().await?;
        return Ok(());
    }

    let request = match timeout(WRAPPER_FIRST_FRAME_TIMEOUT, read_frame_async(&mut stream)).await {
        Ok(Ok(request)) => request,
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            warn!(wrapper = %wrapper_name, peer = ?peer, "wrapper socket first-frame timeout");
            audit::auth_denied(
                "wrapper",
                "timed out waiting for first wrapper request frame",
                Some(peer_uid),
                peer.as_ref().map(|p| p.gid),
            );
            stream.shutdown().await?;
            return Ok(());
        }
    };

    let response = match &request {
        WrapperRequest::Read { scope } => {
            info!(wrapper = %wrapper_name, peer = ?peer, "wrapper read request");
            process_read_for(wrapper_name, scope.as_deref(), &state, peer_uid).await
        }
        WrapperRequest::Unlock { scope, seconds } => {
            info!(wrapper = %wrapper_name, peer = ?peer, "unlock request via wrapper socket");
            process_wrapper_unlock(wrapper_name, scope.as_deref(), *seconds, peer_uid, &state)
        }
        WrapperRequest::UnlockResponse {
            scope,
            seconds,
            challenge,
            signature,
            public_key_blob,
        } => {
            info!(wrapper = %wrapper_name, peer = ?peer, "unlock challenge response");
            process_wrapper_unlock_response(
                wrapper_name,
                scope.as_deref(),
                *seconds,
                peer_uid,
                &state,
                challenge,
                signature,
                public_key_blob,
            )
        }
    };

    write_frame_async(&mut stream, &response).await?;

    // If we sent a Challenge, the client will send the UnlockResponse on
    // the same connection (it can't reconnect — it would lose the wrapper GID).
    if matches!(&response, WrapperResponse::Challenge { .. }) {
        let follow_up = match timeout(
            WRAPPER_UNLOCK_RESPONSE_TIMEOUT,
            read_frame_async(&mut stream),
        )
        .await
        {
            Ok(Ok(request)) => request,
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                warn!(wrapper = %wrapper_name, peer = ?peer, "unlock challenge follow-up timeout");
                stream.shutdown().await?;
                return Ok(());
            }
        };
        let follow_up_response = match &follow_up {
            WrapperRequest::UnlockResponse {
                scope,
                seconds,
                challenge,
                signature,
                public_key_blob,
            } => {
                info!(wrapper = %wrapper_name, peer = ?peer, "unlock challenge response (same connection)");
                process_wrapper_unlock_response(
                    wrapper_name,
                    scope.as_deref(),
                    *seconds,
                    peer_uid,
                    &state,
                    challenge,
                    signature,
                    public_key_blob,
                )
            }
            _ => WrapperResponse::Error {
                code: ErrorCode::Internal,
                message: "expected UnlockResponse after Challenge".to_string(),
            },
        };
        write_frame_async(&mut stream, &follow_up_response).await?;
    }

    // If this is a successful read for a wrapper with scoped channels,
    // create those channels and send one end via SCM_RIGHTS.
    if matches!(&response, WrapperResponse::ReadOk { .. })
        && matches!(&request, WrapperRequest::Read { .. })
        && let Some(wrapper_def) = state.config.wrappers.get(wrapper_name)
    {
        if let Some((credential_ref, access)) =
            wrapper_libsecret_binding(wrapper_def, &state.config.credentials)
        {
            match setup_secretsd_channel(&stream, credential_ref, access, &state).await {
                Ok(()) => {
                    info!(wrapper = %wrapper_name, credential = %credential_ref, "secretsd channel established");
                }
                Err(e) => {
                    error!(wrapper = %wrapper_name, credential = %credential_ref, err = %e, "failed to setup secretsd channel");
                }
            }
        }

        for binding in wrapper_directory_bindings(wrapper_def, &state.config.credentials) {
            match setup_directory_channel(&stream, &binding.credential_ref, binding.access, &state)
                .await
            {
                Ok(()) => {
                    info!(wrapper = %wrapper_name, credential = %binding.credential_ref, path = %binding.target_path, "directory channel established");
                }
                Err(e) => {
                    error!(wrapper = %wrapper_name, credential = %binding.credential_ref, path = %binding.target_path, err = %e, "failed to setup directory channel");
                }
            }
        }
    }

    stream.shutdown().await?;
    Ok(())
}

/// Create a socketpair, send one end to the wrapper via SCM_RIGHTS,
/// and spawn a channel handler for the daemon's end.
async fn setup_secretsd_channel(
    stream: &UnixStream,
    credential_ref: &str,
    access: AccessMode,
    state: &State,
) -> Result<()> {
    use std::os::unix::io::{AsRawFd, FromRawFd};

    // Create socketpair.
    let (daemon_end, wrapper_end) =
        underwrap_protocol::scm_rights::create_socketpair().context("creating socketpair")?;

    // Send the wrapper's end via SCM_RIGHTS on the existing connection.
    // We need the std UnixStream for sendmsg. We borrow the fd without
    // transferring ownership (forget the wrapper to avoid double-close).
    let std_stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(stream.as_raw_fd()) };

    let wrapper_fd = wrapper_end.as_raw_fd();
    let send_result = underwrap_protocol::scm_rights::send_fd(&std_stream, wrapper_fd);

    // Don't let std_stream close the fd — it's owned by tokio.
    std::mem::forget(std_stream);

    send_result.context("sending secretsd channel fd")?;

    // Close wrapper's end — only the wrapper process should have it.
    drop(wrapper_end);

    // Convert daemon's end to tokio async stream and spawn handler.
    daemon_end.set_nonblocking(true)?;
    let daemon_stream = UnixStream::from_std(daemon_end)?;

    let prefix = credential_ref.to_string();
    let store: Arc<dyn underwrap_store::StorageBackend> = state.store_arc();
    let name = credential_ref.to_string();

    tokio::spawn(async move {
        crate::secretsd_channel::run_channel(daemon_stream, store, prefix, name, access).await;
    });

    Ok(())
}

async fn setup_directory_channel(
    stream: &UnixStream,
    credential_ref: &str,
    access: AccessMode,
    state: &State,
) -> Result<()> {
    use std::os::unix::io::{AsRawFd, FromRawFd};

    let (daemon_end, wrapper_end) =
        underwrap_protocol::scm_rights::create_socketpair().context("creating socketpair")?;

    let std_stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(stream.as_raw_fd()) };
    let wrapper_fd = wrapper_end.as_raw_fd();
    let send_result = underwrap_protocol::scm_rights::send_fd(&std_stream, wrapper_fd);
    std::mem::forget(std_stream);
    send_result.context("sending directory channel fd")?;
    drop(wrapper_end);

    daemon_end.set_nonblocking(true)?;
    let daemon_stream = UnixStream::from_std(daemon_end)?;

    let prefix = format!("{credential_ref}/files");
    let store = state.store_arc();
    let credential = credential_ref.to_string();

    tokio::spawn(async move {
        crate::directory_channel::run_channel(daemon_stream, store, prefix, credential, access)
            .await;
    });

    Ok(())
}

async fn process_read_for(
    wrapper_name: &str,
    scope: Option<&str>,
    state: &State,
    peer_uid: u32,
) -> WrapperResponse {
    let wrapper_def = match state.config.wrappers.get(wrapper_name) {
        Some(w) => w,
        None => {
            warn!(wrapper = %wrapper_name, "unknown wrapper");
            return WrapperResponse::Error {
                code: ErrorCode::NotFound,
                message: format!("unknown wrapper '{wrapper_name}'"),
            };
        }
    };

    // Check unlock state based on wrapper policy.
    if wrapper_def.unlock.mode != UnlockMode::AlwaysUnlocked
        && let Err(reason) = state.unlock.check(wrapper_name, scope, peer_uid)
    {
        audit::read_denied(wrapper_name, &reason, Some(peer_uid), scope);
        return WrapperResponse::Error {
            code: ErrorCode::Locked,
            message: reason,
        };
    }

    let mut env = std::collections::HashMap::new();
    for (cred_ref, variable) in wrapper_env_credentials(wrapper_def, &state.config.credentials) {
        match state.store.read(cred_ref) {
            Ok(Some(material)) => {
                audit::read_granted(wrapper_name, cred_ref, Some(peer_uid), scope);
                env.insert(variable.to_string(), material);
            }
            Ok(None) => {
                let msg = format!("credential '{cred_ref}' not found in store");
                audit::read_denied(wrapper_name, &msg, Some(peer_uid), scope);
                return WrapperResponse::Error {
                    code: ErrorCode::NotFound,
                    message: msg,
                };
            }
            Err(e) => {
                error!(credential = %cred_ref, err = %e, "storage read error");
                return WrapperResponse::Error {
                    code: ErrorCode::Internal,
                    message: "internal storage error".to_string(),
                };
            }
        }
    }

    let directories = wrapper_directory_bindings(wrapper_def, &state.config.credentials);
    for binding in &directories {
        audit::read_granted(wrapper_name, &binding.credential_ref, Some(peer_uid), scope);
    }

    WrapperResponse::ReadOk { env, directories }
}

// ---------------------------------------------------------------------------
// Self-service unlock via wrapper socket (with optional SSH agent challenge)
// ---------------------------------------------------------------------------

/// Handle an unlock request according to the wrapper's unlock policy.
fn process_wrapper_unlock(
    wrapper_name: &str,
    scope: Option<&str>,
    seconds: Option<u64>,
    peer_uid: u32,
    state: &State,
) -> WrapperResponse {
    let wrapper_def = match state.config.wrappers.get(wrapper_name) {
        Some(w) => w,
        None => {
            return WrapperResponse::Error {
                code: ErrorCode::NotFound,
                message: format!("unknown wrapper '{wrapper_name}'"),
            };
        }
    };

    match wrapper_def.unlock.mode {
        UnlockMode::AlwaysUnlocked => WrapperResponse::Error {
            code: ErrorCode::Internal,
            message: format!("wrapper '{wrapper_name}' does not require unlock"),
        },
        UnlockMode::Always | UnlockMode::Optional => {
            let max = wrapper_def
                .unlock
                .max_seconds
                .expect("validated maxSeconds");
            if let Some(requested) = seconds
                && requested > max
            {
                return WrapperResponse::Error {
                    code: ErrorCode::Unauthorized,
                    message: format!(
                        "requested unlock duration {requested}s exceeds maxSeconds {max}s"
                    ),
                };
            }

            if wrapper_def.unlock_key.is_some() {
                let nonce: [u8; 32] = rand::random();
                state
                    .unlock
                    .set_pending_challenge(wrapper_name, scope, peer_uid, &nonce);

                WrapperResponse::Challenge {
                    nonce: nonce.to_vec(),
                }
            } else {
                state.unlock.unlock(wrapper_name, scope, seconds, peer_uid);
                audit::unlocked(wrapper_name, scope, seconds, Some(peer_uid));
                WrapperResponse::UnlockOk
            }
        }
    }
}

/// Handle the second round of unlock: verify the SSH signature over the challenge.
#[allow(clippy::too_many_arguments)]
fn process_wrapper_unlock_response(
    wrapper_name: &str,
    scope: Option<&str>,
    seconds: Option<u64>,
    peer_uid: u32,
    state: &State,
    challenge: &[u8],
    signature: &[u8],
    public_key_blob: &[u8],
) -> WrapperResponse {
    let wrapper_def = match state.config.wrappers.get(wrapper_name) {
        Some(w) => w,
        None => {
            return WrapperResponse::Error {
                code: ErrorCode::NotFound,
                message: format!("unknown wrapper '{wrapper_name}'"),
            };
        }
    };

    // Verify the configured unlock key matches the one the caller used.
    let expected_key = match &wrapper_def.unlock_key {
        Some(k) => k,
        None => {
            return WrapperResponse::Error {
                code: ErrorCode::Internal,
                message: "wrapper has no unlock_key configured".to_string(),
            };
        }
    };

    // Parse and compare public keys.
    let expected_pk = match ssh_key::PublicKey::from_openssh(expected_key) {
        Ok(k) => k,
        Err(e) => {
            error!(wrapper = %wrapper_name, err = %e, "invalid configured unlock_key");
            return WrapperResponse::Error {
                code: ErrorCode::Internal,
                message: "invalid configured unlock_key".to_string(),
            };
        }
    };

    let caller_pk = match ssh_key::PublicKey::from_bytes(public_key_blob) {
        Ok(k) => k,
        Err(e) => {
            warn!(wrapper = %wrapper_name, err = %e, "invalid public key from caller");
            return WrapperResponse::Error {
                code: ErrorCode::Unauthorized,
                message: "invalid public key".to_string(),
            };
        }
    };

    if expected_pk.key_data() != caller_pk.key_data() {
        audit::auth_denied("unlock", "public key mismatch", Some(peer_uid), None);
        return WrapperResponse::Error {
            code: ErrorCode::Unauthorized,
            message: "public key does not match configured unlock_key".to_string(),
        };
    }

    // Verify the challenge was issued for this wrapper+scope+uid.
    if !state
        .unlock
        .verify_pending_challenge(wrapper_name, scope, peer_uid, challenge)
    {
        return WrapperResponse::Error {
            code: ErrorCode::Unauthorized,
            message: "invalid or expired challenge nonce".to_string(),
        };
    }

    // Verify the SSH signature over the challenge nonce.
    let ssh_sig = match ssh_key::Signature::try_from(signature) {
        Ok(s) => s,
        Err(e) => {
            warn!(err = %e, "failed to parse SSH signature");
            return WrapperResponse::Error {
                code: ErrorCode::Unauthorized,
                message: "invalid SSH signature format".to_string(),
            };
        }
    };

    use signature::Verifier;
    let valid = match caller_pk.key_data() {
        KeyData::Ed25519(key) => key.verify(challenge, &ssh_sig).is_ok(),
        KeyData::Rsa(key) => key.verify(challenge, &ssh_sig).is_ok(),
        _ => {
            return WrapperResponse::Error {
                code: ErrorCode::Unauthorized,
                message: "unsupported key type (only ed25519 and rsa supported)".to_string(),
            };
        }
    };

    if !valid {
        audit::auth_denied(
            "unlock",
            "signature verification failed",
            Some(peer_uid),
            None,
        );
        return WrapperResponse::Error {
            code: ErrorCode::Unauthorized,
            message: "signature verification failed".to_string(),
        };
    }

    // Signature valid — grant the unlock.
    state.unlock.unlock(wrapper_name, scope, seconds, peer_uid);
    audit::unlocked(wrapper_name, scope, seconds, Some(peer_uid));
    WrapperResponse::UnlockOk
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::process_control;
    use crate::peer_auth::{PeerCred, check_peer_auth};
    use crate::startup::{paths_overlap, validate_relative_credential_path};
    use crate::status::credential_stored;
    use crate::wrapper_bindings::{
        wrapper_directory_bindings, wrapper_env_credentials, wrapper_libsecret_binding,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;
    use underwrap_protocol::config::CredentialMode;

    struct MemStore {
        data: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MemStore {
        fn with_entries(entries: &[(&str, &[u8])]) -> Self {
            Self {
                data: Mutex::new(
                    entries
                        .iter()
                        .map(|(k, v)| ((*k).to_string(), v.to_vec()))
                        .collect(),
                ),
            }
        }
    }

    impl StorageBackend for MemStore {
        fn read(&self, secret_id: &str) -> Result<Option<SecretBytes>> {
            Ok(self
                .data
                .lock()
                .unwrap()
                .get(secret_id)
                .cloned()
                .map(Into::into))
        }

        fn write(&self, secret_id: &str, material: &[u8]) -> Result<()> {
            self.data
                .lock()
                .unwrap()
                .insert(secret_id.to_string(), material.to_vec());
            Ok(())
        }

        fn list(&self, prefix: &str) -> Result<Vec<String>> {
            let mut ids: Vec<_> = self
                .data
                .lock()
                .unwrap()
                .keys()
                .filter(|id| id.starts_with(prefix))
                .cloned()
                .collect();
            ids.sort();
            Ok(ids)
        }

        fn delete(&self, secret_id: &str) -> Result<bool> {
            Ok(self.data.lock().unwrap().remove(secret_id).is_some())
        }
    }

    fn env_credential(variable: &str) -> underwrap_protocol::config::CredentialDef {
        underwrap_protocol::config::CredentialDef {
            mode: CredentialMode::Env {
                variable: variable.to_string(),
            },
        }
    }

    fn libsecret_credential() -> underwrap_protocol::config::CredentialDef {
        underwrap_protocol::config::CredentialDef {
            mode: CredentialMode::Libsecret {},
        }
    }

    fn directory_credential(path: &str) -> underwrap_protocol::config::CredentialDef {
        underwrap_protocol::config::CredentialDef {
            mode: CredentialMode::Directory {
                path: path.to_string(),
            },
        }
    }

    fn wrapper_with_uses(uses: &[(&str, underwrap_protocol::config::CredentialUse)]) -> WrapperDef {
        WrapperDef {
            target: "/usr/bin/true".into(),
            use_map: uses
                .iter()
                .map(|(name, usage)| ((*name).to_string(), usage.clone()))
                .collect(),
            unlock: underwrap_protocol::config::UnlockPolicy::default(),
            socket: "/tmp/test.sock".into(),
            group: "test-group".to_string(),
            unlock_key: None,
        }
    }

    fn state_with_config_and_store(
        wrappers: HashMap<String, WrapperDef>,
        credentials: HashMap<String, underwrap_protocol::config::CredentialDef>,
        allowed_user_uids: std::collections::HashSet<u32>,
        store: MemStore,
    ) -> State {
        State {
            config: DaemonConfig {
                sockets: underwrap_protocol::config::SocketConfig::default(),
                storage: underwrap_protocol::config::StorageConfig::Plain {
                    dir: "/tmp/underwrap-test".into(),
                },
                allowed_users: Vec::new(),
                credentials,
                wrappers,
            },
            store: Arc::new(store),
            allowed_user_uids,
            unlock: UnlockState::new(),
        }
    }

    #[test]
    fn validate_relative_credential_path_accepts_clean_relative_paths() {
        assert!(validate_relative_credential_path(".config/gh").is_ok());
        assert!(validate_relative_credential_path("dir/subdir/file").is_ok());
    }

    #[test]
    fn validate_relative_credential_path_rejects_bad_inputs() {
        for bad in ["", "/abs", ".", "..", "a//b", "a/./b", "a/../b", "a\0b"] {
            assert!(
                validate_relative_credential_path(bad).is_err(),
                "expected rejection for {bad:?}"
            );
        }
    }

    #[test]
    fn paths_overlap_detects_identical_and_nested_paths() {
        assert!(paths_overlap("a/b", "a/b"));
        assert!(paths_overlap("a", "a/b"));
        assert!(paths_overlap("a/b", "a"));
        assert!(!paths_overlap("a/b", "a-b"));
        assert!(!paths_overlap("a/b", "a/c"));
    }

    #[test]
    fn check_peer_auth_covers_allow_all_root_uid_uid_and_gid() {
        assert!(check_peer_auth(&None, &[], &[]).is_ok());

        let root = Some(PeerCred {
            pid: 1,
            uid: 0,
            gid: 999,
            egid: Some(999),
        });
        assert!(check_peer_auth(&root, &[1000], &[1001]).is_ok());

        let by_uid = Some(PeerCred {
            pid: 2,
            uid: 1000,
            gid: 2000,
            egid: Some(2000),
        });
        assert!(check_peer_auth(&by_uid, &[1000], &[]).is_ok());

        let by_gid = Some(PeerCred {
            pid: 3,
            uid: 3000,
            gid: 2000,
            egid: Some(2000),
        });
        assert!(check_peer_auth(&by_gid, &[], &[2000]).is_ok());

        let denied = check_peer_auth(&by_gid, &[1234], &[5678]).unwrap_err();
        assert!(denied.contains("not in allowlist"));
        assert!(check_peer_auth(&None, &[1234], &[]).is_err());
    }

    #[test]
    fn wrapper_binding_helpers_select_expected_credentials() {
        let credentials = HashMap::from([
            ("github/token".to_string(), env_credential("GH_TOKEN")),
            ("libsecret/default".to_string(), libsecret_credential()),
            (
                "gws/state".to_string(),
                directory_credential(".config/gcloud"),
            ),
        ]);
        let wrapper = wrapper_with_uses(&[
            (
                "libsecret/default",
                underwrap_protocol::config::CredentialUse {
                    access: AccessMode::ReadWrite,
                },
            ),
            (
                "github/token",
                underwrap_protocol::config::CredentialUse {
                    access: AccessMode::ReadOnly,
                },
            ),
            (
                "gws/state",
                underwrap_protocol::config::CredentialUse {
                    access: AccessMode::ReadWrite,
                },
            ),
        ]);

        assert_eq!(
            wrapper_libsecret_binding(&wrapper, &credentials),
            Some(("libsecret/default", AccessMode::ReadWrite))
        );

        assert_eq!(
            wrapper_env_credentials(&wrapper, &credentials),
            vec![("github/token", "GH_TOKEN")]
        );

        assert_eq!(
            wrapper_directory_bindings(&wrapper, &credentials),
            vec![ReadDirectoryBinding {
                credential_ref: "gws/state".to_string(),
                target_path: ".config/gcloud".to_string(),
                access: AccessMode::ReadWrite,
            }]
        );
    }

    #[test]
    fn credential_stored_checks_env_libsecret_and_directory_namespaces() {
        let store = MemStore::with_entries(&[
            ("github/token", b"ghp_test"),
            ("libsecret/default/item_1", b"secret"),
            ("gws/state/files/access_tokens.json", b"{}"),
        ]);

        assert!(credential_stored("github/token", &env_credential("GH_TOKEN"), &store).unwrap());
        assert!(credential_stored("libsecret/default", &libsecret_credential(), &store).unwrap());
        assert!(
            credential_stored("gws/state", &directory_credential(".config/gcloud"), &store)
                .unwrap()
        );

        assert!(!credential_stored("missing/env", &env_credential("MISSING"), &store).unwrap());
        assert!(!credential_stored("missing/libsecret", &libsecret_credential(), &store).unwrap());
        assert!(
            !credential_stored(
                "missing/dir",
                &directory_credential(".config/missing"),
                &store
            )
            .unwrap()
        );
    }

    #[test]
    fn process_control_status_all_and_status_target_cover_wrapper_and_credential() {
        let wrappers = HashMap::from([(
            "gh-authorized".to_string(),
            wrapper_with_uses(&[(
                "github/token",
                underwrap_protocol::config::CredentialUse {
                    access: AccessMode::ReadOnly,
                },
            )]),
        )]);
        let credentials = HashMap::from([("github/token".to_string(), env_credential("GH_TOKEN"))]);
        let state = state_with_config_and_store(
            wrappers,
            credentials,
            std::collections::HashSet::new(),
            MemStore::with_entries(&[("github/token", b"ghp_test")]),
        );

        let resp = process_control(&ControlRequest::StatusAll, &state, Some(1000));
        assert_eq!(
            resp,
            ControlResponse::StatusAllOk {
                wrappers: vec!["gh-authorized".to_string()],
                credentials: vec!["github/token".to_string()],
            }
        );

        let resp = process_control(
            &ControlRequest::StatusTarget {
                target: "gh-authorized".to_string(),
            },
            &state,
            Some(1000),
        );
        match resp {
            ControlResponse::StatusWrapperOk { status } => {
                assert_eq!(status.wrapper, "gh-authorized");
                assert_eq!(status.usage.len(), 1);
                assert_eq!(status.usage[0].stored, Some(true));
            }
            other => panic!("expected StatusWrapperOk, got {other:?}"),
        }

        let resp = process_control(
            &ControlRequest::StatusTarget {
                target: "github/token".to_string(),
            },
            &state,
            Some(1000),
        );
        match resp {
            ControlResponse::StatusCredentialOk { status } => {
                assert_eq!(status.credential, "github/token");
                assert!(status.stored);
            }
            other => panic!("expected StatusCredentialOk, got {other:?}"),
        }
    }

    #[test]
    fn process_control_unlock_enforces_authorization_rules() {
        let mut wrapper = wrapper_with_uses(&[(
            "github/token",
            underwrap_protocol::config::CredentialUse {
                access: AccessMode::ReadOnly,
            },
        )]);
        wrapper.unlock = underwrap_protocol::config::UnlockPolicy {
            mode: UnlockMode::Always,
            max_seconds: Some(30),
        };

        let base_wrappers = HashMap::from([("locked-w".to_string(), wrapper)]);
        let base_credentials =
            HashMap::from([("github/token".to_string(), env_credential("GH_TOKEN"))]);

        let state = state_with_config_and_store(
            base_wrappers.clone(),
            base_credentials.clone(),
            std::collections::HashSet::from([0]),
            MemStore::with_entries(&[]),
        );

        let resp = process_control(
            &ControlRequest::Unlock {
                target: "locked-w".to_string(),
                scope: None,
                seconds: Some(60),
                grant_user: None,
                grant_all: false,
            },
            &state,
            Some(1000),
        );
        match resp {
            ControlResponse::Error { code, message } => {
                assert_eq!(code, ErrorCode::Unauthorized);
                assert!(message.contains("exceeds maxSeconds"));
            }
            other => panic!("expected Error, got {other:?}"),
        }

        let resp = process_control(
            &ControlRequest::Unlock {
                target: "locked-w".to_string(),
                scope: None,
                seconds: Some(10),
                grant_user: Some("root".to_string()),
                grant_all: false,
            },
            &state,
            Some(1000),
        );
        match resp {
            ControlResponse::Error { code, message } => {
                assert_eq!(code, ErrorCode::Unauthorized);
                assert!(message.contains("only root may use --for-user or --for-all"));
            }
            other => panic!("expected Error, got {other:?}"),
        }

        let resp = process_control(
            &ControlRequest::Unlock {
                target: "locked-w".to_string(),
                scope: None,
                seconds: Some(10),
                grant_user: None,
                grant_all: false,
            },
            &state,
            Some(0),
        );
        match resp {
            ControlResponse::Error { code, message } => {
                assert_eq!(code, ErrorCode::Unauthorized);
                assert!(message.contains("root must specify --for-user or --for-all"));
            }
            other => panic!("expected Error, got {other:?}"),
        }

        let not_allowed_state = state_with_config_and_store(
            base_wrappers,
            base_credentials,
            std::collections::HashSet::from([123456]),
            MemStore::with_entries(&[]),
        );
        let resp = process_control(
            &ControlRequest::Unlock {
                target: "locked-w".to_string(),
                scope: None,
                seconds: Some(10),
                grant_user: Some("root".to_string()),
                grant_all: false,
            },
            &not_allowed_state,
            Some(0),
        );
        match resp {
            ControlResponse::Error { code, message } => {
                assert_eq!(code, ErrorCode::Unauthorized);
                assert!(message.contains("is not allowed for this instance"));
            }
            other => panic!("expected Error, got {other:?}"),
        }

        let grant_all_state = state_with_config_and_store(
            HashMap::from([("locked-w".to_string(), {
                let mut wrapper = wrapper_with_uses(&[(
                    "github/token",
                    underwrap_protocol::config::CredentialUse {
                        access: AccessMode::ReadOnly,
                    },
                )]);
                wrapper.unlock = underwrap_protocol::config::UnlockPolicy {
                    mode: UnlockMode::Always,
                    max_seconds: Some(30),
                };
                wrapper
            })]),
            HashMap::from([("github/token".to_string(), env_credential("GH_TOKEN"))]),
            std::collections::HashSet::from([0]),
            MemStore::with_entries(&[]),
        );
        let resp = process_control(
            &ControlRequest::Unlock {
                target: "locked-w".to_string(),
                scope: Some("session-1".to_string()),
                seconds: Some(10),
                grant_user: None,
                grant_all: true,
            },
            &grant_all_state,
            Some(0),
        );
        assert_eq!(resp, ControlResponse::UnlockOk);
        assert!(
            grant_all_state
                .unlock
                .check("locked-w", Some("session-1"), 0)
                .is_ok()
        );
        assert!(
            grant_all_state
                .unlock
                .check("locked-w", Some("session-1"), 1000)
                .is_err()
        );
    }

    #[test]
    fn process_control_lock_and_store_rejections() {
        let mut wrapper = wrapper_with_uses(&[(
            "gws/state",
            underwrap_protocol::config::CredentialUse {
                access: AccessMode::ReadOnly,
            },
        )]);
        wrapper.unlock = underwrap_protocol::config::UnlockPolicy {
            mode: UnlockMode::Always,
            max_seconds: Some(30),
        };

        let state = state_with_config_and_store(
            HashMap::from([("locked-w".to_string(), wrapper)]),
            HashMap::from([
                ("github/token".to_string(), env_credential("GH_TOKEN")),
                (
                    "gws/state".to_string(),
                    directory_credential(".config/gcloud"),
                ),
            ]),
            std::collections::HashSet::new(),
            MemStore::with_entries(&[]),
        );

        let resp = process_control(
            &ControlRequest::Unlock {
                target: "locked-w".to_string(),
                scope: Some("session-1".to_string()),
                seconds: Some(10),
                grant_user: None,
                grant_all: false,
            },
            &state,
            Some(1000),
        );
        assert_eq!(resp, ControlResponse::UnlockOk);
        assert!(
            state
                .unlock
                .check("locked-w", Some("session-1"), 1000)
                .is_ok()
        );

        let resp = process_control(
            &ControlRequest::Lock {
                target: Some("locked-w".to_string()),
                scope: Some("session-1".to_string()),
            },
            &state,
            Some(1000),
        );
        assert_eq!(resp, ControlResponse::LockOk);
        assert!(
            state
                .unlock
                .check("locked-w", Some("session-1"), 1000)
                .is_err()
        );

        let resp = process_control(
            &ControlRequest::Store {
                secret_id: "gws/state".to_string(),
                material: "data".into(),
            },
            &state,
            Some(1000),
        );
        match resp {
            ControlResponse::Error { code, message } => {
                assert_eq!(code, ErrorCode::Unauthorized);
                assert!(message.contains("not an env credential"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
