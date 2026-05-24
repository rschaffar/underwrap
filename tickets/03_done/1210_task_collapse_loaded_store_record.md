# Task: Drop `LoadedStoreRecord`; collapse parallel maps to `StoreEntry`

Resolves: B-ADM-13, B-ADM-18, B-COR-3, B-COR-5 (partial), B-COR-6,
B-COR-15.

## Problem

Three related representational accidents in the store registry.

1. **Zero-value newtype** (B-COR-3):
   `crates/backend-backendd/src/store_registry.rs:26-30`:
   ```rust
   pub struct LoadedStoreRecord { pub record: StoreRecord }
   ```
   One field, no methods, no invariants. Every caller writes
   `.record.foo` immediately.

2. **Parallel maps that must be kept in sync** (B-COR-3):
   `crates/backend-backendd/src/store_registry.rs:70-73`:
   ```rust
   StoreRegistry {
       state:   HashMap<String, StoreState>,
       records: HashMap<String, LoadedStoreRecord>,
   }
   ```
   Every CRUD path in `admin_api/stores/crud.rs` must mutate both
   maps in lockstep. The data model permits inconsistency.

3. **Live-state-lookup boilerplate** (B-ADM-13):
   `crates/backend-backendd/src/admin_api/stores/crud.rs:301-313,354-371`
   and `stores/lifecycle.rs:84-94,424-438` each repeat:
   ```rust
   let reg = state.stores.read().unwrap();
   let loaded = reg.records.get(&name).ok_or_else(AdminError::store_not_found)?;
   match reg.state.get(&name) {
       Some(StoreState::Unlocked(b)) => …,
       Some(StoreState::Locked) => …,
       None => return Err(AdminError::internal_with_code("internal",
           "store metadata present but registry state missing")),
   }
   ```

4. **Rotation lock leak** (B-ADM-18):
   The `rotation_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>` grows
   unbounded; deleting a store leaves its rotation lock alive
   (`admin_api/stores/lifecycle.rs:35-42`).

5. **Dead helpers** (B-COR-5, B-COR-15):
   - `StoreState::is_locked()` (inherent method, `store_registry.rs:42-53`)
     has no non-test callers.
   - `StoreRegistry::is_locked(store_id) -> bool`
     (`store_registry.rs:100-103`) swallows NotFound, only test-callers.
   - `StoreRegistry::new()` is only called from `test_support.rs`.

6. **Unused return value** (B-COR-6):
   `StoreRegistry::unlock` docstring promises a useful return; the
   sole caller (`admin_api/stores/lifecycle.rs:514-516`) ignores it.

## What to do

1. Delete the `LoadedStoreRecord` newtype.
2. Collapse the registry to one map:
   ```rust
   pub struct StoreEntry {
       pub record: StoreRecord,
       pub state: StoreState,
       pub rotation_lock: Arc<Mutex<()>>,  // dies with the entry
   }
   pub struct StoreRegistry {
       pub entries: HashMap<String, StoreEntry>,
   }
   ```
3. Drop `state.rotation_locks` from `AdminRuntimeState` entirely;
   the lock now lives on the entry.
4. Add a helper:
   ```rust
   pub fn admin_store_state(&self, id: &str)
       -> Result<&StoreEntry, RegistryLookupError>;
   ```
   Returns the entry or a typed lookup error. The four
   live-state-lookup sites collapse to a single match.
5. Change `StoreRegistry::unlock` return type to `()`; drop the
   doc paragraph about returning the previous state.
6. Delete `StoreState::is_locked()` (inherent), `StoreRegistry::is_locked(...)`,
   and `StoreRegistry::new()`. Tests use struct literals or the
   `admin_store_state` helper.
7. Update all CRUD paths in `admin_api/stores/crud.rs`,
   `stores/lifecycle.rs`, and any other readers to use the
   collapsed shape.

## Acceptance

- `cargo test --workspace`
- `cargo test -p integration-tests`
- `nix build .#checks.x86_64-linux.vm-test`
- `cargo clippy --all-targets --workspace -- -D warnings`
