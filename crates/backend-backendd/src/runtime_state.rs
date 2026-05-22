//! Shared in-memory state used by both the runtime (mTLS) handlers and
//! the admin (browser-facing) handlers.
//!
//! There is intentionally exactly one of these per running daemon,
//! wrapped in an `Arc`. Every request handler resolves whatever it
//! needs (store registry, client-instance registry, session key,
//! mutexes, ...) by reaching into this struct rather than threading
//! the individual pieces through the call chain.
//!
//! Fields are `pub(crate)` so the modules under `admin_api/` and
//! `runtime_api/` can read them directly.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::store_registry::StoreRegistry;

pub(crate) struct AdminRuntimeState {
    /// Open store backends + their backend-managed records. Shared
    /// with the service listener so runtime requests and admin CRUD
    /// always consult the same live registry without a daemon restart.
    pub(crate) stores: Arc<std::sync::RwLock<StoreRegistry>>,
    /// Runtime client-instance registry, shared with the service
    /// listener so admin mutations can affect new runtime requests
    /// without a daemon restart.
    pub(crate) client_instances:
        Arc<std::sync::RwLock<HashMap<String, crate::runtime_clients::ClientInstanceRecord>>>,
    /// Path of the backend-managed store-registry file.
    pub(crate) stores_file: PathBuf,
    /// Path of the backend-managed runtime client-instance registry file.
    pub(crate) client_instances_file: PathBuf,
    /// Directory under which the admin "create store" API places the
    /// data directory of each new dynamic store. The on-disk layout is
    /// `<stores_data_root>/<store_id>/`; the admin chooses only the
    /// store's display name. `None` disables creation (returns
    /// `data_root_not_configured`).
    pub(crate) stores_data_root: Option<PathBuf>,
    /// Per-store rotation lock. Held for the duration of
    /// `rotate-key` so that concurrent writes through the runtime
    /// service do not land in the pre-swap data directory. Lazily
    /// populated on first use; entries are never removed (the cost is
    /// one `Mutex` per known store, which is negligible).
    pub(crate) rotation_locks: std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Whether the browser-facing admin UI/API is enabled on the HTTPS
    /// listener. When false, `/v1/admin/*` and SPA routes return 404.
    pub(crate) admin_enabled: bool,
    /// When present, every state-changing request must carry an `Origin`
    /// (or `Referer`) matching one of these entries.
    pub(crate) allowed_origins: Option<Vec<String>>,
    /// Where to read the SPA bundle from when serving non-API paths.
    pub(crate) web_source: crate::web::WebAssetSource,
    /// Path of the backend-managed operator-state file.
    pub(crate) users_file: PathBuf,
    /// 32-byte AEAD key used to encrypt session cookies.
    pub(crate) session_key: [u8; crate::auth::SESSION_KEY_LEN],
    /// Configured session lifetime in seconds.
    pub(crate) session_lifetime_secs: u64,
    /// Whether to mark issued cookies with the `Secure` attribute.
    /// True when the admin listener terminates TLS.
    pub(crate) cookies_secure: bool,
    /// Per-username login attempt limiter.
    pub(crate) login_rate_limiter: crate::auth::RateLimiter,
    /// Server-side set of revoked session ids. Logout adds the session
    /// id here so a stolen-but-still-unexpired cookie is rejected on
    /// the next request.
    pub(crate) session_revocations: crate::auth::SessionRevocations,
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
