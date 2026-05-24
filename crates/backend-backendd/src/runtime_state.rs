//! Listener-shared state used by both the runtime (mTLS) handlers and
//! the admin (browser-facing) handlers.
//!
//! There is exactly one of these per running daemon, wrapped in an
//! `Arc`. It carries:
//!
//! - [`SharedState`] — the store registry and runtime client
//!   collection, consulted by every request the listener serves.
//! - [`AdminState`] — the browser-facing-only fields (session store,
//!   CSRF config, on-disk paths, cross-request mutexes, …). Present
//!   iff the admin UI/API is enabled for this listener.
//!
//! Fields are `pub(crate)` so the modules under `admin_api/` and
//! `runtime_api/` can reach into them directly.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum_csrf::CsrfConfig;
use tower_sessions::MemoryStore;

use crate::store_registry::StoreRegistry;

/// State consulted by *every* request the listener handles. The runtime
/// (mTLS) handlers see nothing else; admin handlers reach for this
/// through [`ListenerState::shared`].
pub(crate) struct SharedState {
    /// Open store backends + their backend-managed records. Shared
    /// with the service listener so runtime requests and admin CRUD
    /// always consult the same live registry without a daemon restart.
    pub(crate) stores: Arc<std::sync::RwLock<StoreRegistry>>,
    /// Runtime client-instance registry, shared with the service
    /// listener so admin mutations can affect new runtime requests
    /// without a daemon restart.
    pub(crate) client_instances:
        Arc<std::sync::RwLock<HashMap<String, crate::runtime_clients::ClientInstanceRecord>>>,
}

/// State used by the browser-facing admin handlers only. Lives in
/// [`ListenerState::admin`] as an `Option`; absent when the listener
/// runs in runtime-only mode.
pub(crate) struct AdminState {
    /// Directory holding one JSON file per backend-managed store
    /// (`<stores_dir>/<id>.json`). The admin CRUD handlers persist
    /// here; startup reads it via [`crate::store_registry::load_store_records`].
    pub(crate) stores_dir: PathBuf,
    /// Directory holding one JSON file per runtime client-instance
    /// record (`<clients_dir>/<id>.json`).
    pub(crate) clients_dir: PathBuf,
    /// Directory under which the admin "create store" API places the
    /// data directory of each new dynamic store. The on-disk layout is
    /// `<stores_data_root>/<store_id>/`; the admin chooses only the
    /// store's display name. `None` disables creation (returns
    /// `data_root_not_configured`).
    pub(crate) stores_data_root: Option<PathBuf>,
    /// When present, every state-changing request must carry an `Origin`
    /// (or `Referer`) matching one of these entries.
    pub(crate) allowed_origins: Option<Vec<String>>,
    /// Directory holding one JSON file per admin operator
    /// (`<users_dir>/<id>.json`).
    pub(crate) users_dir: PathBuf,
    /// Path of the persistent bootstrap-completed sentinel file. Its
    /// presence means the online bootstrap flow has already run.
    pub(crate) bootstrap_file: PathBuf,
    /// `axum-login` backend resolving credentials and user ids against
    /// the operator collection. Cloning shares the per-username
    /// login-rate-limiter state, so callers should clone this and not
    /// construct fresh instances.
    pub(crate) auth_backend: crate::auth::AuthBackend,
    /// In-memory session store handed to `tower-sessions`. Held here
    /// so request handlers can address the store directly when
    /// needed.
    pub(crate) session_store: MemoryStore,
    /// `axum_csrf` config — cookie name, attributes, HMAC salt. The
    /// CSRF layer reads this; admin handlers also consult it when
    /// emitting fresh tokens. Always built from [`Self::cookies_secure`]
    /// at construction time so the two stay in sync.
    pub(crate) csrf_config: CsrfConfig,
    /// Configured session lifetime in seconds. Used as the
    /// `Max-Age` on the session and CSRF cookies and as the
    /// inactivity expiry on the session store.
    pub(crate) session_lifetime_secs: u64,
    /// Whether to mark issued cookies with the `Secure` attribute.
    /// True when the admin listener terminates TLS. The CSRF config
    /// stored alongside is built from this same flag.
    pub(crate) cookies_secure: bool,
    /// Mutex that serialises bootstrap attempts so concurrent callers
    /// can't both pass the "is operator collection empty" check
    /// before either has written the first record. The lock is held
    /// across the read-collection / write-collection / mark-sentinel
    /// sequence.
    pub(crate) bootstrap_lock: std::sync::Mutex<()>,
    /// Mutex that serialises operator-collection read-modify-write
    /// cycles so two concurrent admin requests (e.g. two user
    /// renames) can't observe and overwrite each other's intermediate
    /// state. Held across `load` → mutate → `save`.
    pub(crate) user_collection_lock: std::sync::Mutex<()>,
}

/// Top-level listener state. Wraps the always-live [`SharedState`] and
/// the optional admin-only [`AdminState`]. Handlers extract this via
/// `State<Arc<ListenerState>>`.
pub(crate) struct ListenerState {
    pub(crate) shared: SharedState,
    pub(crate) admin: Option<AdminState>,
}

impl ListenerState {
    /// `true` when the listener exposes the browser-facing admin
    /// UI/API. Drives the `/v1/admin/*` and SPA-route 404 gate.
    pub(crate) fn admin_enabled(&self) -> bool {
        self.admin.is_some()
    }

    /// Borrow the [`AdminState`], panicking if absent. Safe inside
    /// any handler that is wired only into the admin router — those
    /// routes are gated by [`Self::admin_enabled`] at the middleware
    /// boundary, so reaching them with no `AdminState` would be a
    /// programming error.
    pub(crate) fn admin(&self) -> &AdminState {
        self.admin
            .as_ref()
            .expect("admin route reached without admin state")
    }
}
