# Task: `StoreRegistry` carries lock state per store

Parent: `500_story_in_memory_master_keys.md`
Epic: `epic_in_memory_master_keys.md`

## Scope

- In `crates/backend-backendd/src/server.rs`, change `StoreRegistry`:
  ```rust
  pub enum StoreState {
      Unlocked(Arc<dyn StorageBackend>),
      Locked,
  }

  pub struct StoreRegistry {
      pub state:   HashMap<String, StoreState>,
      pub records: HashMap<String, LoadedStoreRecord>,
  }
  ```
  Replace every existing `open` field reference accordingly.
- Add `impl StoreRegistry`:
  - `pub fn unlocked_backend(&self, store_id: &str) -> Result<Arc<dyn StorageBackend>, RegistryLookupError>`
    where `RegistryLookupError` is a small enum
    `{ NotFound, Locked }`. Used by every service / admin call site so
    no handler pattern-matches on `StoreState` directly.
  - `pub fn is_locked(&self, store_id: &str) -> bool` for the admin
    list / detail views.
  - `pub fn lock(&mut self, store_id: &str) -> bool` (transition,
    drops the `Arc<dyn StorageBackend>`).
  - `pub fn unlock(&mut self, store_id: &str, backend: Arc<dyn StorageBackend>)`.
- Startup behaviour in `run()` (around `open_stores`):
  - Plain stores: open as today, insert as `Unlocked`.
  - Encrypted stores: do **not** attempt to construct the backend.
    Insert as `Locked`. `open_stores` becomes
    `open_plain_stores` (rename); a new helper materialises locked
    placeholders for encrypted records.
  - Drop the `keys_dir` field from `AdminRuntimeState` and from the
    CLI / NixOS module knobs (`crates/backend-backendd/src/config.rs`
    and `nix/modules/underwrap-backend.nix` if a knob is still there).
- Migrate every existing call site that reads
  `state.stores.read().open[&id]` to
  `state.stores.read().unlocked_backend(&id)?`. Translate the new
  error variants per the call-site contract (admin endpoints map to
  `StoreLocked`; the service layer translation happens in ticket 530).
- Add unit tests for the registry helpers (lock / unlock /
  unlocked_backend on locked record / on unknown record).

## Acceptance criteria

- [ ] All references to `StoreRegistry::open` are gone.
- [ ] Plain stores are always queryable; encrypted stores return
      `RegistryLookupError::Locked` until unlocked.
- [ ] `keys_dir` is removed from `AdminRuntimeState` and from the
      `BackendConfig` loader.
- [ ] Existing tests that previously seeded an encrypted store on
      startup now either seed a plain store or call the
      not-yet-existent unlock endpoint (mark such tests `#[ignore]`
      until 540 lands — or use the registry helper directly in a
      tests-only constructor).
- [ ] `cargo test -p underwrap-backendd` builds. Some test names may
      be flagged for follow-up tickets.

## Out of scope

- The new `ErrorCode::StoreLocked` (530).
- Wire mapping in HTTP responses (530).
- The unlock / lock endpoints themselves (540).
