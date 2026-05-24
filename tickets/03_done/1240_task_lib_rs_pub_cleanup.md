# Task: Strip `lib.rs` re-exports; tighten module visibility

Resolves: B-COR-1, B-COR-5 (remainder), B-COR-9, B-COR-10, B-COR-11,
B-COR-12, B-COR-13, B-COR-14, B-RT-5.

## Problem

Several pieces of unused or over-broad public surface across the
`backend-backendd` crate root.

### 1. `lib.rs` re-exports nobody consumes (B-COR-1)

`crates/backend-backendd/src/lib.rs:23-33`:

```rust
pub use crate::admin_api::auth::bootstrap::{BootstrapRequest, BootstrapResponse, ...};
pub use crate::admin_api::auth::{LoginRequest, LoginResponse, WhoamiResponse};
pub use crate::config::{AdminConfig, BackendConfig, DEFAULT_STORES_DIR, HttpsConfig, HttpsTlsConfig};
pub use crate::store_registry::{LoadedStoreRecord, RegistryLookupError, StoreRegistry,
    StoreState, is_valid_store_id, load_store_records};
```

Workspace-wide grep for `underwrap_backendd::{Bootstrap*, Login*,
Whoami*, DEFAULT_STORES_DIR, LoadedStoreRecord, RegistryLookupError,
StoreRegistry, StoreState, is_valid_store_id, load_store_records}`
finds zero external consumers. The actual crate-public surface used
by integration-tests, the binary, and `tests/openapi_export.rs` is:
`BackendConfig`, `HttpsConfig`, `HttpsTlsConfig`, `AdminConfig`,
`cli::*`, `server::run`, `openapi::admin_openapi_spec`,
`store_registry::save_store_record`, `runtime_clients::*`.

### 2. Over-broad `pub mod` declarations

`lib.rs:2,16-18`: `pub mod auth`, `pub mod web`, `pub mod tls`,
`pub mod users` — none used externally.

### 3. Dead helpers

- **`cert_fingerprint_from_pem_file`** (B-COR-5): zero callers.
  `crates/backend-backendd/src/runtime_clients.rs:95-99`.
- **`generate_resource_id`** (B-RT-5):
  `crates/backend-backendd/src/runtime_api/wrappers.rs:275-280` is
  a one-line `crate::ids::random_id("res")` wrapper with one caller
  and a stale "mirrors generate_client_instance_id" comment (no such
  function exists).
- **`recover_user` vacuous invariant check** (B-COR-10):
  `crates/backend-backendd/src/cli.rs:21-52` lines 47-49 re-use
  `UserCollection::add` "for its uniqueness + hash invariants" but
  both checks are vacuously true at that call site (dup already
  excluded by `find_by_username`; hash just produced by `auth::hash_password`).

### 4. `Body<T>` field visibility mismatch (B-COR-14)

`crates/backend-backendd/src/http/responses.rs:27`:
```rust
pub(crate) struct Body<T>(pub T);
```
Type is `pub(crate)`, field is `pub`. Inconsistent.

### 5. Manual `Debug for UserCollection` (B-COR-9)

`crates/backend-backendd/src/users.rs:101-110` carries a manual impl
with a comment saying "we implement Debug here to keep the same
redaction-by-default posture for the wrapper" — but `#[derive(Debug)]`
on `UserCollection` would use `UserRecord::Debug` automatically, which
already redacts.

### 6. `load_bootstrap_sentinel` is test-only (B-COR-12)

`crates/backend-backendd/src/users.rs:376-387`. Production reads only
`bootstrap_completed(&path) -> bool`; the sentinel's `completed_at` /
`by_username` fields are write-only on disk.

### 7. Misplaced helpers (B-COR-13)

`crates/backend-backendd/src/users.rs:216-229` defines
`now_epoch_secs` and `new_user_id`. The former is imported as
`users::now_epoch_secs` from `runtime_api/wrappers.rs`,
`runtime_api/invocations.rs`, `admin_api/resources.rs`,
`admin_api/pairs.rs` — none of which deal with users.

### 8. Duplicate `storage::documents` load/save helpers (B-COR-11)

`crates/backend-backendd/src/storage/documents.rs:25-85` has six
functions of identical shape (`load_policy_document`,
`load_resources_document`, `load_pairs_document`, plus three `save_*`).

## What to do

1. Delete the entire `pub use ...` block at `lib.rs:23-33`. No
   external consumer relies on these. (Coordinate with 1140 which
   demotes the underlying `pub struct LoginRequest` etc. to
   `pub(crate)` — the `pub use` of a `pub(crate)` item is legal as
   long as the re-export doesn't escape.)
2. Demote `pub mod auth`, `pub mod web`, `pub mod tls`,
   `pub mod users` to `pub(crate) mod ...` in `lib.rs`.
3. Keep `pub mod cli`, `pub mod config`, `pub mod openapi`,
   `pub mod server`, `pub mod store_registry`,
   `pub mod runtime_clients` (the actually-used surface).
4. Rewrite the comment "External consumers (integration-tests, the
   ts_export binding test, and the binary itself) reach for these
   types via the crate root" so it matches the new reality.
5. Delete `cert_fingerprint_from_pem_file` (B-COR-5).
6. Delete `generate_resource_id`; inline `crate::ids::random_id("res")`
   at its one call site (B-RT-5).
7. Delete lines 47-49 of `cli.rs` (`recover_user`) and the misleading
   "Re-use UserCollection::add" comment; save the record directly
   (B-COR-10).
8. Fix `Body<T>` field visibility to `pub(crate)` (B-COR-14).
9. Delete the manual `Debug for UserCollection` impl; derive `Debug`
   (B-COR-9).
10. Decide `load_bootstrap_sentinel` (B-COR-12): either delete and
    write a presence-only sentinel file, or surface its contents via
    `GET /v1/admin/bootstrap-status`. Pick presence-only unless the
    SPA can make use of the metadata immediately.
11. Move `now_epoch_secs` to a new `crate::time` module (or to
    `ids.rs`). Decide on `new_user_id` — keep in `users.rs` if it's
    a user-specific factory; rely on `ids::random_id` for the
    underlying generator (B-COR-13).
12. Collapse `storage::documents` to two generics (B-COR-11):
    ```rust
    pub(crate) fn load_document<T: Default + DeserializeOwned>(
        store: &dyn StorageBackend, secret_id: &str,
    ) -> anyhow::Result<T>;

    pub(crate) fn save_document<T: Serialize>(
        store: &dyn StorageBackend, secret_id: &str, doc: &T,
    ) -> anyhow::Result<()>;
    ```
    Keep three thin per-document aliases if call sites read better
    that way.

## Acceptance

- `cargo build -p backend-backendd`
- `cargo build -p integration-tests`
- `cargo test --workspace`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cargo fmt -- --check`
