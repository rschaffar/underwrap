# Task: Consolidate admin_api shared helpers

Resolves: B-ADM-4, B-ADM-5, B-ADM-6, B-ADM-7, B-ADM-14, B-ADM-15,
B-ADM-16, B-ADM-17, B-ADM-24, B-ADM-25.

## Problem

Several small patterns are duplicated or open-coded across the
admin_api/ subtree.

## What to do

1. **Password validation** (B-ADM-4):
   Extract `fn validate_password_or_bad_request(password: &str) ->
   Result<(), AdminError>` returning the canonical `password_empty` /
   `password_too_short` codes. Use it in all four sites:
   - `crates/backend-backendd/src/admin_api/users.rs:205-214,296-305`
   - `crates/backend-backendd/src/admin_api/auth/mod.rs:211-220`
   - `crates/backend-backendd/src/admin_api/auth/bootstrap.rs:145-155`
   Per-site cosmetic message variations go away; codes stay.

2. **Audit `result_label`** (B-ADM-7):
   Move `audit_result` from
   `crates/backend-backendd/src/admin_api/libsecret.rs:140-142` into
   `crate::admin_api::audit` as
   `pub(crate) fn result_label<T>(r: &Result<T, AdminError>) ->
   &'static str`. Use it in:
   - `admin_api/secrets.rs:182-186,232-236`
   - `admin_api/resources.rs:295-296`
   - `admin_api/users.rs:128-156`
   …and any other open-coded `if outcome.is_ok() { "success" } else
   { "failure" }`.

3. **`saturating_u32`** (B-ADM-6):
   Promote `saturating_u32(value: usize) -> u32` from
   `admin_api/stores/mod.rs:59-61` to a shared `admin_api::util`
   module (or `storage::documents` next to `ENTITY_DEPENDENT_SAMPLE_LIMIT`).
   Use it at:
   - `admin_api/pairs.rs:177`
   - `admin_api/resources.rs:178,194`
   - `storage/documents.rs:95`

4. **`policy_permission_dependents`** (B-ADM-5):
   Add a helper next to `resource_secret_dependents` in
   `crate::storage::documents`:
   ```rust
   pub(crate) fn policy_permission_dependents(
       store: &dyn StorageBackend,
       predicate: impl Fn(&WrapperPermission) -> bool,
   ) -> anyhow::Result<(u32, Vec<AdminWrapperPermissionKey>)>
   ```
   Use it from both:
   - `admin_api/pairs.rs:165-179`
   - `admin_api/resources.rs:182-196`

5. **`IntoInternal` adapter** (B-ADM-25):
   ```rust
   pub(crate) trait IntoInternal {
       fn into_internal(self, ctx: &'static str) -> AdminError;
   }
   impl IntoInternal for anyhow::Error { ... }
   ```
   Use across handlers so `load_pairs_document(...).map_err(|e|
   e.into_internal("loading pairs document"))?` replaces the
   `map_err(|e| AdminError::internal("...", e))` boilerplate.
   Module-level comment in `error.rs` about "deliberately not implemented
   From<anyhow::Error>" stays accurate — the trait makes the intent
   explicit without giving up the context-string invariant.

6. **Fold `body_b64` into `util`** (B-ADM-24):
   The module's name doesn't match its contents (two base64 helpers,
   nothing about HTTP bodies). Move the two functions into the new
   `admin_api::util` module; delete `body_b64.rs`.

7. **`reencrypt_into` boilerplate compression** (B-ADM-14):
   `crates/backend-backendd/src/admin_api/stores/lifecycle.rs:297-380`
   has five error branches each doing
   `let _ = std::fs::remove_dir_all(stage_dir); + log_admin_event(...)
   + return Err(AdminError::internal_with_code(...))`. Extract a
   `bail!` closure capturing `stage_dir`, `principal`, `store_name`,
   and use it. Same pattern in the enclosing
   `handle_admin_store_rotate_key`.

8. **`parse_slot` arms** (B-ADM-15):
   `crates/backend-backendd/src/admin_api/secrets.rs:55-70` —
   collapse:
   ```rust
   Some(s @ (ResourceSlot::Env | ResourceSlot::Files)) => Ok(s),
   ```

9. **`force` query** (B-ADM-16):
   `crates/backend-backendd/src/admin_api/stores/crud.rs:322-326,349-352`
   — change the schema from `force: Option<String>` to `force: bool`
   with a serde default; drop the empty-string-as-truthy branch.

10. **Redundant `#[serde(default)]` on `Option<T>`** (B-ADM-17):
    Sites:
    - `admin_api/secrets.rs:72-78` (`resource_id: Option<String>`)
    - `admin_api/stores/crud.rs:322-326` (covered by force-bool above)
    Delete the `#[serde(default)]` attribute on `Option<T>` fields —
    serde already defaults `Option` to `None`.

## Acceptance

- `cargo test --workspace`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cargo fmt -- --check`
