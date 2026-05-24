# Task: Decompose `handle_runtime_wrapper_register`

Resolves: B-RT-3, B-RT-4, B-RT-6, B-RT-7.

## Problem

`crates/backend-backendd/src/runtime_api/wrappers.rs:32-273` is a
240-line handler entangling three flows:

1. Input validation.
2. Loading three documents.
3. Computing the missing-resources/missing-pairs diff.
4. The no-op fast path with permission seeding.
5. The auto-create-disabled refusal.
6. The auto-create commit with three sequential saves + policy seeding.

Six exit points return the same `internal` envelope. The
`seed_locked_permissions` closure at `:122-148` is a 27-line
`FnMut`-shaped block mutating `&mut PolicyDocument` from outer scope
— a poor man's free function (B-RT-4).

Adjacent issues:

- **Duplicate existence check** (B-RT-7):
  `crates/backend-backendd/src/runtime_api/principal.rs:65-73`
  does `state.stores.read().unwrap().records.contains_key(&store_id)`,
  then `crates/backend-backendd/src/runtime_api/mod.rs:76-95` re-takes
  the lock and calls `unlocked_backend(&store_id)` for the same fact.
  Two lock acquisitions, two existence checks.
- **Cross-place invariant** (B-RT-6):
  `validate_runtime_client_mappings` (`principal.rs:22-43`) enforces
  "every client instance points to an existing store" at startup;
  `admin_api/stores/crud.rs:421` (store-delete handler) must
  maintain the same invariant. The constraint lives in two places.

## What to do

1. Decompose `handle_runtime_wrapper_register` into pure helpers:
   ```rust
   fn compute_register_diff(
       body: &RuntimeWrapperRegisterRequest,
       resources: &ResourcesDocument,
       pairs: &WrapperResourcePairsDocument,
   ) -> RegisterDiff;

   fn apply_auto_create(
       resources: &mut ResourcesDocument,
       pairs: &mut WrapperResourcePairsDocument,
       diff: &RegisterDiff,
       creator: EntityCreator,
   ) -> (Vec<String>, Vec<RuntimeWrapperRegisterPair>);

   fn seed_locked_permissions(
       policy: &mut PolicyDocument,
       body: &RuntimeWrapperRegisterRequest,
       resolved: &[...],
       principal: &Principal,
   ) -> bool;
   ```
   Plus a driver that loads documents, dispatches the three outcomes
   (no-op / refusal / commit), and saves. End state: ~80 lines of
   orchestration + small unit-testable pieces.
2. Add unit tests for each helper (covers shapes the integration
   tests don't exercise individually).
3. **Drop the duplicate existence check** (B-RT-7): delete the
   `contains_key` precheck in `resolve_runtime_client`. Let
   `unlocked_backend`'s `NotFound` arm carry the "unknown store"
   log. (After 1210 this is trivially safe — `StoreEntry` is the
   single source of truth.)
4. **Cross-place invariant comment** (B-RT-6): in
   `validate_runtime_client_mappings`, add a WHY-comment pointing at
   the admin store-delete handler that must maintain the same
   invariant. Out of scope to redesign the data model; this is
   documentation-of-existing-coupling only.

## Acceptance

- `cargo test --workspace`
- `cargo test -p integration-tests` — wire shape unchanged
- `nix build .#checks.x86_64-linux.vm-test`
- `cargo clippy --all-targets --workspace -- -D warnings`
