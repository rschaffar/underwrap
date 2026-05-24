# Task: Lift `json_dir_store<T>` generic for the three on-disk registries

Resolves: B-COR-2, B-COR-4.

## Problem

Three modules in `backend-backendd` each implement a near-identical
on-disk-records pattern:

- `crates/backend-backendd/src/users.rs:232-364`
- `crates/backend-backendd/src/store_registry.rs:179-313`
- `crates/backend-backendd/src/runtime_clients.rs:101-280`

Each carries:
- `record_path(dir, id)`
- `load_one(path)` with id-from-filename backfill
- `load(dir)`: read_dir + `.json` filter + sort + uniqueness scan
- `save_*` with tempfile + write_all + sync_all + persist
- NotFound-tolerant `delete_*`

Down to identical `with_context` formatting and identical anyhow
error wording. The only per-entity logic is: validate id, validate
record fields, dedupe key.

Three single-line wrappers also each redirect to the same id validator
(`ids::is_valid_simple_id`):

- `users.rs:420-422` (`is_valid_user_id`)
- `store_registry.rs:318-320` (`is_valid_store_id`)
- `runtime_clients.rs:282-284` (`is_valid_client_instance_id`)

## What to do

1. Add a private `crate::storage::json_dir_store` module exposing:
   ```rust
   pub(crate) trait DirRecord: Serialize + DeserializeOwned + Sized {
       fn id(&self) -> &str;
       fn set_id(&mut self, id: String);
       fn validate(&self, path: &Path) -> anyhow::Result<()>;
       fn uniqueness_keys(&self) -> impl Iterator<Item = (&'static str, String)>;
   }

   pub(crate) fn load_dir<T: DirRecord>(dir: &Path) -> anyhow::Result<Vec<T>> { ... }
   pub(crate) fn save<T: DirRecord>(dir: &Path, record: &T) -> anyhow::Result<()> { ... }
   pub(crate) fn delete(dir: &Path, id: &str) -> anyhow::Result<()> { ... }
   ```
   The `set_id` method handles the `#[serde(default, skip)] pub id`
   round-trip (the contract is now in one place — covers B-COR-27 from
   the review).
2. Implement `DirRecord` for `UserRecord`, `StoreRecord`,
   `ClientInstance`. Each domain module's loader/saver/deleter
   becomes a one-line forwarder (or disappears entirely if call
   sites can use the generic directly).
3. Drop the three `is_valid_<thing>_id` wrappers (B-COR-4). Either
   call `ids::is_valid_simple_id` directly, or rename it to
   `ids::is_valid_id` since the `simple` qualifier is unused
   (recommended).
4. The three domain modules shrink to ~30 lines each plus their
   typed mutation/validation logic.

## Acceptance

- `cargo test --workspace`
- Round-trip property: load/save/delete via the generic produces
  byte-identical on-disk JSON to the pre-refactor implementation
  (add a snapshot test if one doesn't exist).
- `cargo clippy --all-targets --workspace -- -D warnings`
- `nix build .#checks.x86_64-linux.vm-test` (sanity: VM still boots
  and reads existing store/user records correctly).
