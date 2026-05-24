# Task: Split `AdminRuntimeState` god-struct

Resolves: B-COR-7, B-COR-8, S-10.

## Problem

`crates/backend-backendd/src/runtime_state.rs:22-93` defines a single
`AdminRuntimeState` that fuses two listeners' worth of state:

- **Shared (always live):** `stores`, `client_instances`.
- **Admin-only (twelve fields, all unread when `admin_enabled = false`):**
  `session_store`, `auth_backend`, `csrf_config`, `session_lifetime_secs`,
  `cookies_secure`, `bootstrap_lock`, `user_collection_lock`,
  `allowed_origins`, `users_dir`, `bootstrap_file`, `stores_data_root`,
  `rotation_locks` (the last one will move onto `StoreEntry` in
  `1210_task_collapse_loaded_store_record`).

The type name (`AdminRuntimeState`) and its docstring describe "the
admin and runtime state" — both the structure and the name muddle the
fact that the runtime API needs none of the admin fields.

Also nearby:

- **`Principal` in the wrong module** (B-COR-8):
  `crates/backend-backendd/src/users.rs:89-93` defines
  `Principal { id, username }`. `users.rs`'s rustdoc is about
  on-disk records; `Principal` is a request-context concept consumed
  only by `admin_api/*` and resolved by `admin_api::csrf::resolve_principal_layer`.

- **`cookies_secure` wired twice with hardcoded `true`** (S-10):
  `crates/backend-backendd/src/server.rs:62,66`:
  ```rust
  csrf_config: crate::admin_api::csrf::build_csrf_config(true),
  // ...
  cookies_secure: true,
  ```
  Two co-dependent flags, set at the same site, not linked.

## What to do

1. Split into two structs:
   ```rust
   pub struct SharedState {
       pub stores: Arc<RwLock<StoreRegistry>>,
       pub client_instances: Arc<RwLock<ClientInstances>>,
   }

   pub struct AdminState {
       pub session_store: ...,
       pub auth_backend: ...,
       pub csrf_config: ...,
       pub session_lifetime_secs: u64,
       pub cookies_secure: bool,
       pub bootstrap_lock: Mutex<()>,
       pub user_collection_lock: Mutex<()>,
       pub allowed_origins: Vec<String>,  // see 1230 if not already required
       pub users_dir: PathBuf,
       pub bootstrap_file: PathBuf,
       pub stores_data_root: Option<PathBuf>,
   }
   ```
2. Carry the listener state as one of:
   - `enum ListenerState { RuntimeOnly(Arc<SharedState>),
     WithAdmin { shared: Arc<SharedState>, admin: Arc<AdminState> } }`
   - or `Arc<SharedState>` + `Option<Arc<AdminState>>` on a small
     outer struct.
   Pick whichever reads cleaner at the listener-construction site
   in `server.rs`.
3. Rename `AdminRuntimeState` to `ListenerState` (or equivalent) —
   the old name was already misleading.
4. Wire `csrf_config` *from* `cookies_secure` in one place (S-10).
   Either build the config at `AdminState` construction time using
   `cookies_secure`, or change `build_csrf_config` to take
   `cookies_secure: bool` and call it once with the same value used
   on the session layer.
5. Move `Principal` from `crate::users` to `crate::admin_api::csrf`
   (where the layer that produces it lives) — or to
   `crate::admin_api::mod` if that reads cleaner. Update imports.

## Acceptance

- `cargo test --workspace`
- `cargo test -p integration-tests`
- `nix build .#checks.x86_64-linux.vm-test`
- `cargo clippy --all-targets --workspace -- -D warnings`
- A grep for `cookies_secure` shows it set in exactly one place that
  feeds both the session layer and `build_csrf_config`.
