# Store backend migration plan

## Goal

Allow an authenticated operator to migrate one existing store between plain and encrypted storage from the Admin UI without changing its stable store id, metadata, runtime client mappings, control documents, resources, or logical bytes.

Require exact key re-entry before encrypted-store creation or plain-to-encrypted migration performs any storage mutation.

## Context

The current Admin UI creates either a plain or encrypted store but cannot change the backend kind later.

Encrypted creation currently generates the key during the mutating create request and requires only an acknowledgement checkbox after creation.

Key rotation has a separately hardened retained-generation workflow and remains outside this feature except where shared lifecycle serialization prevents overlap.

The user requested this feature on 2026-09-07 and selected hard pre-mutation key confirmation for both encrypted creation and plain-to-encrypted migration.

That request grants implementation authority after this plan is committed.

Implementation must stop for a new dependency, weaker security boundary, changed rotation API or UX, unresolved recovery disposition, or materially different API contract.

## Plan Relationship

No active plan overlaps this work.

The completed F-003 rotation work and its retained-generation documentation remain historical and current-state evidence rather than implementation authority.

This plan preserves rotation's existing API, one-time post-mutation key reveal, recovery generations, and ordinary-I/O quiescence limitation.

## Architecture

### Admin contract

| Operation | Request | Success | Principal errors |
| --- | --- | --- | --- |
| `POST /v1/admin/store-keys` | Empty body | `200 { key_hex }` with `Cache-Control: no-store, no-cache, must-revalidate`, `Pragma: no-cache`, and `Expires: 0` | Standard authentication and CSRF errors |
| `POST /v1/admin/stores` | `{ name, backend, description?, key_hex? }` | `200 { store }` | `400 key_required`, `unexpected_key`, or `invalid_key_hex`, plus existing create errors |
| `POST /v1/admin/stores/{id}/migrate` | `{ backend, key_hex? }` | `200 { store }` | `400 key_required`, `unexpected_key`, or `invalid_key_hex`; `404 not_found`; `409 same_backend`; `503 store_locked` or `store_busy`; and the exact `500` codes below |

Encrypted creation and plain-to-encrypted migration require one canonical 64-character lowercase hexadecimal key and do not trim or normalize it.

Plain creation and encrypted-to-plain migration reject any supplied key.

Existing unlock keeps its current surrounding-whitespace tolerance and does not reuse the stricter create/migrate parser accidentally.

Plain-to-encrypted success leaves the store `unlocked` with the submitted key installed.

Encrypted-to-plain success leaves the store `ready` and needs no key.

Migration failures use this fixed public mapping.

| Failure disposition | HTTP status and code |
| --- | --- |
| Workspace or manifest reservation fails before displacement | `500 migrate_stage_failed` |
| Source listing fails before displacement | `500 migrate_list_failed` |
| Source or target canary validation fails before displacement | `500 migrate_canary_failed` |
| A source value cannot be read before displacement | `500 migrate_read_failed` |
| A target value cannot be written before displacement | `500 migrate_write_failed` |
| The staged or promoted id set or logical bytes do not verify | `500 migrate_verify_failed` |
| Source displacement, target promotion, or checked rollback rename fails | `500 migrate_swap_failed` when a verified source is restored, otherwise `500 migration_recovery_required` |
| The promoted target cannot be reopened | `500 migrate_reopen_failed` when a verified source is restored, otherwise `500 migration_recovery_required` |
| Store-record persistence fails | `500 write_store_registry_failed` when a verified source and source record are restored, otherwise `500 migration_recovery_required` |
| Secret-bearing workspace cleanup partially or wholly fails at any phase | `500 migration_recovery_required` |
| Existing leases do not drain within 30 seconds | `503 store_busy` with the unchanged source restored |
| Startup finds an exact migration workspace | `500 migration_recovery_required` for storage access until trusted-host recovery |

All public messages are fixed and value-free.

A phase-specific error is returned only when rollback and cleanup leave one verified source authoritative and no ambiguous split state.

Add a redacted, zeroizing Rust key-string type whose Serde and OpenAPI projections remain ordinary strings.

Use that owned type for new key-generation, creation, and migration DTOs without changing rotation or unlock parsing semantics.

### Browser key confirmation

The supported browser first requests a fresh backend-generated key without mutating storage.

The browser displays that key, keeps it outside TanStack query and mutation caches, and requires exact untrimmed re-entry.

A mismatch sends no mutation request and keeps the generated key available for another attempt.

Cancellation before any mutation request clears the component state and leaves storage unchanged.

After a mutation request is sent, any error or lost response is potentially ambiguous.

The browser retains the displayed generated key, clears request and re-entry copies where practical, refreshes the store list for reconciliation, and permits retry with the same key.

The operator can explicitly discard the retained key only after a warning that it may already protect committed or recoverable data.

Confirmed success clears the generated and re-entry state and does not reveal a second key.

Browser re-entry proves only an exact in-browser comparison and does not prove response delivery, external custody, independent recoverability, or operator behavior.

### Lifecycle states and actions

Replace the list projection's combined backend-kind and `locked` interpretation with an explicit lifecycle state.

| Lifecycle state | Meaning | Allowed store actions |
| --- | --- | --- |
| `ready` | Plain backend available | Edit, migrate to encrypted, and existing usage-dependent delete |
| `unlocked` | Encrypted backend and key available | Edit, migrate to plain, rotate, lock, and existing usage-dependent delete |
| `locked` | Encrypted backend without a key | Edit, unlock, and existing conservative force-delete |
| `busy` | Migration owns lifecycle publication | No mutation action |
| `recovery-required` | Durable migration state needs trusted-host recovery | No mutation action |

Expose `can_update` and `can_migrate` alongside the existing action fields so the UI does not infer valid mutations from backend kind.

Locked, busy, and recovery-required usage keeps the current conservative no-content-inspection semantics and never reports inspected emptiness.

Runtime and admin storage requests receive `503 store_busy` while the entry is busy.

Recovery-required storage requests receive `500 migration_recovery_required` rather than being presented as an ordinary locked store.

### Lifecycle admission and lock order

Rename the entry's rotation mutex to a lifecycle mutex and acquire it before migration or rotation admission.

Migration takes the lifecycle mutex, then briefly takes the registry lock to re-resolve the exact entry and atomically replace its available state with `busy`.

No registry lock is held across `.await`, lease drainage, storage I/O, filesystem mutation, or registry-file persistence.

Every normal production backend lookup returns a scoped lease that covers the complete handler storage operation.

Publishing `busy` under the registry write lock prevents every later lease, while leases admitted earlier decrement an entry-owned count and notify the migration waiter on drop.

Migration waits for that exact entry's admitted leases to drain before reading the source or mutating the filesystem.

A cancellation guard restores the source only if the same entry identity, lifecycle mutex, source generation, and busy admission still match.

After drainage, the synchronous copy, verification, filesystem, persistence, cleanup, and publication window runs inside a runtime-aware `block_in_place` boundary on the production multi-thread runtime and contains no asynchronous cancellation point.

Rotation acquires the shared lifecycle mutex but preserves its current non-draining behavior for ordinary reads and writes.

Update, delete, lock, unlock, rotation, and migration revalidate the exact entry and state under their final registry lock so none can publish over `busy`, `recovery-required`, or a replacement entry.

Real delayed admin and runtime handler tests complement registry-unit lease tests, and a bounded production-source inventory must find no naked backend clone that bypasses leases.

An invocation that opened before migration but finalizes later remains subject to the existing stateless and last-writer-wins limitations.

Operators must therefore quiesce wrapper traffic and admin mutations for the selected store before migration.

### Migration workspace and publication

Use a collision-safe exclusive mode-`0700` sibling workspace named `.<store-id>.underwrap-migration-<16-lowercase-hex-id>`.

The workspace contains `stage/`, later `backup/`, and a non-secret versioned manifest containing only the stable store id and source and target backend kinds.

Create both encrypted and plain target roots exclusively as regular non-symlink directories with exact mode `0700`, including under a permissive umask, and reject every setuid, setgid, sticky, or other extra mode bit.

Require every canonical, stage, backup, rollback, and cleanup path to satisfy checked occupancy, directory, and symlink rules before a rename or removal.

Retain the source id inventory without a whole-store plaintext map, copy and drop one logical value at a time through the source and target `StorageBackend` implementations, and verify and drop one source/target pair at a time while checking complete id sets and every logical value byte-for-byte.

While the original source remains live, retain only a bounded domain-separated SHA-256 fingerprint over its kind, canary treatment, and complete sorted length-delimited id/value sequence alongside that id inventory.

Treat the digest as rollback-integrity evidence without claiming collision impossibility or protection from malicious backend-host root.

Plain-to-encrypted migration rejects an unexpected source `control/canary` and writes and verifies the canonical encrypted canary in the target.

Encrypted-to-plain migration fingerprints and verifies but omits the source canary from the plain target.

After target verification, move the source to `backup/`, securely reopen and require that displaced generation's fingerprint through the same `StorageBackend` kind, promote `stage/` at the unchanged canonical path, require the fingerprint before and after moving a target aside and after restoring the source, reopen and reverify the target, atomically persist the same store record with only its backend kind changed, remove the displaced source and workspace, publish the target live entry, and return success.

Any source fingerprint mismatch, unreadable source, or source id mismatch requires recovery and preserves the workspace rather than being reported as target verification failure.

A reported successful plain-to-encrypted migration retains no intentional plaintext source or stage pathname.

Path removal is not secure media erasure and does not establish that prior plaintext blocks cannot be recovered from the storage medium.

### Directional plaintext residue

| Direction | Interrupted or failed residue |
| --- | --- |
| Plain to encrypted | The displaced plaintext source can remain under `backup/` after displacement, rollback failure, registry failure, cleanup failure, or interruption |
| Encrypted to plain | The plaintext target can remain under `stage/` or at the canonical path after staging, promotion, cleanup failure, or interruption |

Both residue classes remain mode-`0700`, are outside runtime and admin resource APIs, force recovery-required state, and require trusted-host handling.

The UI warns before migration that failure or interruption can leave plaintext recovery material on the trusted backend host.

### Failure and recovery

Before source displacement, remove only newly created migration state and restore the verified source entry only after that cleanup succeeds.

A pre-displacement cleanup failure keeps the source data intact but publishes recovery-required because secret-bearing target material remains outside the active store.

After source displacement but before registry persistence, attempt checked rollback to the source generation and verify it before restoring availability.

If registry persistence fails, keep or restore the persisted source record, roll back the filesystem when possible, and never publish the target in memory.

After registry persistence, the target record is durable under the existing atomic-visibility but non-power-loss guarantee.

Any failed rollback, failed cleanup of secret-bearing migration state, interrupted split state, or discovered migration workspace places the store in recovery-required state, preserves every remaining artifact without further automatic deletion, and identifies at least one previously verified generation for trusted-host reverification.

Recursive cleanup can partially remove an obsolete generation before failing, so recovery never promises that every pre-cleanup file remains intact.

Startup detects exact store-associated migration workspaces before opening a plain store or classifying an encrypted store as ordinarily locked.

Trusted-host recovery runs with the backend stopped, inventories and preserves every generation, selects and verifies one source or target generation with its matching backend kind and key when encrypted, aligns the registry record, quarantines rather than overwrites unselected state, restarts, verifies representative control and resource reads and runtime client mappings, and only then deletes explicitly selected obsolete artifacts.

Recovery does not promise restoration after media loss, arbitrary corruption, a missing encrypted-generation key, or unflushed power loss.

## Security Invariants

The protected assets are brokered resource confidentiality, integrity, and availability across backend-kind conversion.

The backend, backend-host root, and authenticated operators remain trusted, while runtime clients and local wrapper callers gain no new authority.

Runtime requests continue to use wrapper and resource vocabulary and cannot name a store, storage key, or `control/*` path.

Keys never enter argv, environment variables, logs, audit fields, manifests, query strings, durable configuration, browser storage, or query and mutation caches.

Owned Rust key buffers use redacted formatting and zeroize on drop where practical.

JavaScript strings, HTTP and serialization buffers, clipboard state, allocator remnants, filesystem blocks, and browser internals have no comprehensive zeroization guarantee.

No source is displaced until a complete target has passed target-backend reads.

No success is returned until the target record is persisted, the live target is publishable, and displaced plaintext source paths have been removed.

A returned failure leaves either a verified source authoritative or an explicit recovery-required state.

Migration workspaces are the only new backend-host paths outside the active store and are never reachable through admin resource APIs or runtime APIs.

This feature adds no transaction, compare-and-swap, secure-erasure, parent-directory fsync, power-loss durability, HSM, TPM, escrow, or external-vault claim.

## Non-Goals

- Do not add key re-entry to rotation or change rotation key-delivery behavior.
- Do not add automatic recovery, workspace garbage collection, or browser recovery controls.
- Do not add server-side pending keys, hashes, confirmation tokens, escrow, TPM, HSM, or vault integration.
- Do not make the Admin API prove that a submitted key came from its generation endpoint.
- Do not change the encrypted file format, resource schema, policy semantics, runtime API vocabulary, store id, client mappings, or privileged frontend chain.
- Do not add a dependency, NixOS option, module behavior, or compatibility branch for an unreleased earlier API.
- Do not promise zero downtime or safe concurrent writes during migration.

## Assumptions

- Managed store directories and sibling workspaces are on one filesystem and are writable by the backend service identity.
- Operators quiesce wrapper traffic and admin mutations for the selected store before migration.
- Authenticated administrators are trusted to request conversion and handle key material.
- Exact re-entry uses the full 64-character lowercase key without trimming or normalization.
- Recovery-required stores remain unavailable until trusted-host intervention.

## Validation Setup

Before any Rust command that compiles `underwrap-backend`, prepare the exact current-tree embedded SPA artifact.

```bash
system=$(nix eval --impure --raw --expr builtins.currentSystem)
web_dist=$(nix build --no-link --print-out-paths ".#checks.${system}.web-build")
export UNDERWRAP_WEB_DIST="$web_dist"
```

Use `cargo test --workspace --exclude integration-tests` for the direct workspace suite and use `./crates/integration-tests/run` for integration tests so their exact packaged binary prerequisites are always supplied.

Before Playwright, build the exact changed-tree backend and rebuild the SPA in the same evidence sequence.

```bash
UNDERWRAP_WEB_DIST="$web_dist" cargo build -p underwrap-backend
nix develop -c bash -lc 'cd web && npm run build && UNDERWRAP_BACKENDD_BINARY="$PWD/../target/debug/underwrap-backend" npx playwright test e2e/stores.spec.ts'
```

## Plan

- [ ] Add the migration and lifecycle DTOs additively in `crates/underwrap-protocol/src/service/admin.rs`, including the redacted zeroizing key type, exact route shapes, lifecycle states, action fields, and serde/OpenAPI projections.
  Acceptance evidence: `cargo test -p underwrap-protocol service::tests::admin_store_migration_contract` and `cargo check --workspace --exclude integration-tests` pass under the validation setup before existing create fields are removed.

- [ ] Add lifecycle leases, exact entry-generation matching, and ready/unlocked/locked/busy/recovery-required states in `crates/underwrap-backend/src/store_registry.rs`, then migrate every production backend lookup without holding registry locks across work.
  Acceptance evidence: `cargo test -p underwrap-backend store_registry::tests::migration_lease` and `cargo test -p underwrap-backend store_registry::tests::migration_state` pass under the validation setup; `rg -n 'unlocked_backend|state\.backend\(\).*clone|StoreState::Unlocked' crates/underwrap-backend/src --glob '*.rs'` has a reviewed production disposition for every match; and `cargo check --workspace --exclude integration-tests` passes.

- [ ] Share only collision-safe private-workspace and checked-path primitives between rotation and migration, rename the entry mutex to lifecycle vocabulary, and preserve rotation behavior.
  Acceptance evidence: `cargo test -p underwrap-backend admin_api::stores::lifecycle::rotation` and `cargo test -p underwrap-backend admin_api::stores::lifecycle::concurrency_tests` pass with existing rotation response, retention, key-wipe, audit, and ordinary-I/O semantics.

- [ ] Implement `crates/underwrap-backend/src/admin_api/stores/lifecycle/migration.rs` with separate phases and hooks for copy, canary handling, id-set and byte verification, exact mode and topology checks, promotion, reopen, registry persistence, cleanup, rollback, and recovery-required disposition.
  Acceptance evidence: `cargo test -p underwrap-backend admin_api::stores::lifecycle::migration::tests` and `cargo test -p underwrap-backend admin_api::stores::lifecycle::migration::tests::permissive_umask` pass with deterministic filesystem snapshots for both directions and every injected phase failure.

- [ ] Detect exact migration workspaces during startup and publish affected entries only as recovery-required without automatic mutation.
  Acceptance evidence: `cargo test -p underwrap-backend store_registry::tests::migration_workspace` passes for pre-swap, backup, promoted, persisted-target, malformed-manifest, symlink, and multiple-workspace layouts and verifies byte-identical pre/post trees.

- [ ] Wire `POST /v1/admin/store-keys` and `POST /v1/admin/stores/{id}/migrate`, including non-cache key responses, direction-specific key validation, unlocked-source enforcement, lifecycle revalidation, stable store projection, fixed safe errors, and value-free audit fields.
  Acceptance evidence: `cargo test -p underwrap-backend admin_api::stores::lifecycle` and `cargo test -p underwrap-backend admin_api::stores::lifecycle::audit_tests` pass, including delayed real handler leases, all state/error mappings, byte and mapping preservation, and key/resource-value omission from captured diagnostics.

- [ ] Change encrypted creation and all callers in one compile-clean milestone to require the submitted canonical key before directory creation, reject keys for plain creation, return no key, and retain existing registry failure-atomicity semantics.
  Acceptance evidence: `cargo test -p underwrap-backend admin_api::stores::crud` and `cargo test -p underwrap-backend admin_api::stores::lifecycle::tests::submitted_creation_key` pass under the validation setup; invalid requests have byte-identical registry/filesystem snapshots; a valid store unlocks after restart with that key; and `cargo check --workspace --exclude integration-tests` passes.

- [ ] Regenerate and verify the Admin OpenAPI and generated SPA contract for the new routes, lifecycle states, conditional key behavior, exact response sets, session-plus-CSRF protection, and non-cache key response.
  Acceptance evidence: `cargo test -p underwrap-backend --test openapi_export`, `cargo test --workspace --exclude integration-tests`, `cd web && npm run generate:api`, and `cd web && npm run build` pass under the validation setup with reviewed `crates/underwrap-backend/openapi.json` drift.

- [ ] Add direct typed SPA key-generation, create, and migration calls plus a reveal-and-exact-re-entry component that keeps key material out of TanStack caches and handles pre-request cancellation separately from ambiguous post-request failure.
  Acceptance evidence: `cd web && npm test -- src/pages/stores.test.tsx` passes exact ordering/body, untrimmed match, mismatch, cancellation, success clearing, ambiguous failure retention, same-key retry, explicit warned discard, cache/URL/browser-storage absence, and unchanged rotation-flow cases.

- [ ] Add Stores-page migration actions and dialogs for plain-to-encrypted and unlocked encrypted-to-plain conversion, including busy/recovery disabling, unlock-first guidance, quiescence warning, and both directional plaintext-residue warnings.
  Acceptance evidence: `cd web && npm test -- src/pages/stores.test.tsx` passes lifecycle/action rendering, both directions, stable selection, retryable failures, and no key generation for encrypted-to-plain.

- [ ] Add real authenticated HTTPS and Playwright migration scenarios that preserve store id, metadata, client mapping, resources, control documents, nested ids, and logical bytes across both directions.
  Acceptance evidence: `./crates/integration-tests/run tests::store_migration -- --nocapture` and the exact backend-build plus SPA-build plus focused Playwright sequence in Validation Setup pass; direct host-tree inspection finds exact mode `0700` target roots and no retained plaintext migration pathname after successful plain-to-encrypted migration; simulated lost-response cases retain the displayed key and reconcile state.

- [ ] Update `AGENTS.md`, `crates/underwrap-protocol/src/config.rs`, `crates/underwrap-backend-store/src/lib.rs`, `docs/architecture.md`, `docs/security.md`, `docs/backend.md`, `docs/admin-ui.md`, and `docs/testing.md` with the current generation timing, bounded migration-workspace exception, lifecycle contract, commit ordering, quiescence, directional plaintext residue, key re-entry nonclaim, recovery procedure, and evidence limits.
  Acceptance evidence: `git diff --check` passes; all changed Markdown links resolve; `rg -n 'generated.*creation|creation.*generat|rotation workspace|locked: bool|rotation_lock|StoreState::Locked' AGENTS.md crates/underwrap-protocol/src/config.rs crates/underwrap-backend-store/src/lib.rs crates/underwrap-backend/src/store_registry.rs crates/underwrap-backend/src/admin_api/stores docs` has a reviewed disposition for every stale-contract candidate; and published prose remains one sentence per line.

- [ ] Run complete relevant validation and obtain a bounded independent review of the migration boundary before integration.
  Acceptance evidence: `cargo fmt -- --check`, `cargo clippy --all-targets --workspace -- -D warnings`, `cargo test --workspace --exclude integration-tests`, `./crates/integration-tests/run`, `cd web && npm test && npm run test:coverage && npm run build`, the exact changed-tree backend build followed by `nix develop -c bash -lc 'cd web && UNDERWRAP_BACKENDD_BINARY="$PWD/../target/debug/underwrap-backend" npm run test:e2e'`, `nix flake check --all-systems --no-build`, `nix flake check`, `git diff --check`, exact changed-path inspection, bounded sensitive-pattern scanning, and independent review all pass or any unavailable evidence remains open.

## Rollback / Recovery

Before integration, rollback is removal of the feature branch and linked worktree.

A running migration never deletes or overwrites an unverified source generation.

A recoverable failure keeps the source authoritative and removes only newly created target state.

An ambiguous durable split state fails closed as recovery-required and follows the trusted-host procedure documented by this feature.

After release, reverting code does not automatically convert migrated store records or reconcile migration workspaces.

Operators must complete or roll back any recovery-required store with the matching release's documented offline procedure before running a version without migration support.

## Completion Checklist

- [ ] Key generation is authenticated, CSRF-protected, explicitly non-cacheable, and storage-neutral.
- [ ] Encrypted creation and plain-to-encrypted migration perform no mutation before exact browser re-entry.
- [ ] Pre-request cancel and mismatch send no mutation, while ambiguous post-request failure retains the generated key and supports reconciliation.
- [ ] Encrypted-to-plain migration requires an unlocked source and sends no key.
- [ ] Migration preserves store id, metadata, client mappings, every storage id, and every logical byte except backend-specific canary handling.
- [ ] New normal storage access is drained or rejected before migration filesystem mutation.
- [ ] Successful plain-to-encrypted migration retains no plaintext migration pathname and makes no secure-erasure claim.
- [ ] Every returned failure leaves either a verified source live or an explicit recovery-required state with useful paths preserved.
- [ ] Startup fails closed on every durable migration workspace without automatic mutation.
- [ ] Keys and resource values are absent from logs, argv, configuration, manifests, URLs, and durable browser or TanStack storage.
- [ ] OpenAPI, generated TypeScript, SPA, integration tests, security guidance, and recovery documentation agree.
- [ ] Validation passes without dependency, Nix module, VM, rotation UX, or privileged-chain changes.
