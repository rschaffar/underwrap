# Keep failed registry mutations out of live state

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-015
Source themes: C-001

## Outcome

A store-record or runtime-client-record mutation becomes visible to live requests only after its atomic registry-file save or delete reports success. If persistence returns an error, the admin request returns the existing 500 error and the corresponding live registry remains exactly as it was before the request; restoring and reloading the registry observes the same pre-request record.

This preserves the backend's process-visible authorization contract: a failed runtime-client update cannot rotate certificate acceptance, disable or enable the instance, remap it to another store, change auto-creation authority, or publish a new display name until restart. A failed store update/delete cannot rename, redescribe, or remove the live store until restart. Successful mutations retain immediate live effect without a backend restart.

The guarantee is local to one store or runtime-client registry mutation and its existing write lock. It is not a transaction across the store registry, client registry, policy documents, resource storage, or permission cleanup, and it does not claim power-loss durability.

## Scope

- Correct the three F-015 paths that still publish before persistence at clean HEAD `c9960016adfed8a0342c63e343b8cdedf869773d`:
  - `handle_admin_store_update` derives and validates the complete candidate while holding the store-registry write lock, saves that candidate, and only then replaces the live `StoreEntry.record` before releasing the lock. A save error leaves the exact prior record, `StoreState`, backend handle, and per-entry rotation lock live.
  - `handle_admin_store_delete` retains its current client-mapping and content/force prechecks outside the store write lock, then acquires the store-registry write lock, deletes the registry file, and only after success removes the live entry. A delete error leaves the exact live entry available.
  - `handle_admin_client_instance_update` retains its complete validation and client-registry write-lock scope, saves the candidate record, and only after success replaces the live record. A save error leaves display name, status, current store, credential fingerprint/PEM, last-seen value, and `auto_create_wrapper_assets` unchanged.
- Preserve lock ordering. In particular, do not hold the store-registry write lock while acquiring the client-registry lock or inspecting a backing store. Client update already takes `client_instances.write()` before reading stores; introducing the reverse `stores.write() -> client_instances.read()` order would create a deadlock risk. Store-delete's cross-registry and backing-store prechecks therefore remain separate from its short persistence/publication commit window.
- Keep runtime-client permission cleanup after a successful registry commit and after releasing the client-registry write lock. A failed client save performs no old-store permission cascade. A successful remap/delete remains committed even if the existing best-effort stale-permission cleanup warns and fails.
- Preserve the already failure-safe store/client create and client-delete behavior. They currently hide their temporary candidate/removal under an exclusive lock and restore it before releasing the lock when persistence fails. Retain focused regression evidence for those paths, but do not redesign them merely for stylistic uniformity.
- Add deterministic real-filesystem fault evidence without a production fault hook: after seeding valid persisted records, move the selected registry directory aside and place a regular file at its configured path so save/delete fails before commit; restore it before reload and recovery assertions. Tests must restore the directory on every path.
- Add focused handler evidence for each remaining defect:
  - store update failure returns HTTP 500 code `write_store_registry_failed`, leaves complete live and reloaded records unchanged, then succeeds on retry with live/reloaded equality to the requested state;
  - store delete failure returns the same stable code, leaves the exact live entry and reloaded record present, then succeeds on retry with both absent;
  - client update failure returns HTTP 500 code `write_client_instances_failed`, leaves every live and reloaded field unchanged, leaves old-store permissions unchanged, and then succeeds on retry with live/reloaded equality to the requested state;
  - valid input/duplicate/unknown-store/not-found precedence, successful create/update/delete, locked/unlocked state, backend/rotation-lock identity, and post-commit cascade behavior remain covered.
- Add one bounded real authenticated admin-HTTPS/runtime-mTLS regression. On a blocked client-registry path, a request that proposes `Disabled` status together with a new store mapping, certificate, auto-create flag, and display name receives the existing 500; the original credential remains active and accepted under the original mapping, and none of the candidate fields is live. After restoration, the same service/session first retries the credential/mapping/auto-create/display change with `Active` status: the replacement credential becomes accepted and reloadable while the superseded credential is rejected. It then disables and re-enables the replacement in separate successful requests, proving status behavior without the impossible requirement that a disabled replacement authenticate. Representative store update/delete failures additionally leave authenticated list views and reloaded records unchanged and recover on retry.
- Keep test daemons, backend threads, clients, directory blockers, and temporary files bounded and deterministically restored/stopped on success and failure. Apply any hard timeout in a parent process; never cancel and drop an in-progress async cleanup future.
- Document the process-visible persist-before-publish rule, same-registry lock scope, post-commit best-effort permission cascade, and cross-registry/crash limits in `docs/backend.md`, `docs/security.md`, and `docs/testing.md`.
- Correct only the directly relevant `json_dir_store` wording if needed: writes provide atomic replacement visibility after the temporary file is synced, but the helper does not fsync the parent directory and does not establish a general power-loss durability claim.
- Preserve these clean-HEAD revalidation artifacts outside the repository:
  - desired red-before patch `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1140-red-before.patch`, SHA-256 `0e3d097a650e408060929d318fe6d4cde1a70867d16fc01062331075bfb8a898`;
  - desired red-before log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1140-red-before.log`, SHA-256 `d17c45aaca52c0ecb31130ba883dba0472edbf33adc76d499d4ea13be20b6cfa`;
  - clean backend baseline `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1140-baseline.log`, SHA-256 `a7669f68e57a95b9ecbe062ad0adcbe6a9afe066baa080abb8a76b51efb9fa1f`.
  The three desired tests fail with status 101: failed store update exposes the candidate record live, failed store delete removes the live entry, and failed client update exposes the candidate display/status/store/auto-create state live. Restored disk reloads retain the old records. The unmodified backend baseline passes 252 library tests, three binary tests, OpenAPI drift, and doc tests.

Expected implementation paths are limited to:

- `crates/underwrap-backend/src/admin_api/stores/crud.rs`
- `crates/underwrap-backend/src/admin_api/stores/crud/tests.rs`
- `crates/underwrap-backend/src/admin_api/client_instances.rs`
- `crates/underwrap-backend/src/admin_api/client_instances/tests.rs`
- `crates/underwrap-backend/src/storage/json_dir_store.rs` only for directly relevant durability wording
- `crates/underwrap-backend/src/test_support.rs` only if one shared registry-directory blocker is the simplest test shape
- `crates/integration-tests/src/harness/mod.rs`
- `crates/integration-tests/src/harness/pki.rs` only if needed to exercise the existing alternate synthetic credential
- `crates/integration-tests/src/tests/registry_failure_atomicity.rs`
- `crates/integration-tests/src/tests/mod.rs`
- `docs/backend.md`
- `docs/security.md`
- `docs/testing.md`

A narrower path set is preferred if it meets every acceptance criterion. Any approved behavior-preserving cleanup has its own separately reviewed scope and commit before the finding fix.

## Non-goals

- Do not add a transaction, revision, compare-and-swap token, global coordinator, cross-registry lock, or automatic retry. Store-delete's no-mapped-client/content checks and later registry commit remain separate; concurrent client reassignment or backing-store mutation can invalidate a successful check. O-004 and broader coordination remain separate.
- Do not claim crash consistency, power-loss durability, parent-directory fsync, durable unlink, immunity to kernel/filesystem misreporting, or atomicity across disk commit and in-memory assignment. If persistence succeeds and the process then terminates before publication, restart loads the committed record; this task governs returned helper errors and the running process's observable state.
- Do not repair, reconcile, or migrate pre-existing live/disk divergence. Do not scan for or synthesize missing records.
- Do not change store/client create semantics, clean up a data directory or encrypted canary left by failed store creation, or redesign generated-key custody. This task corrects registry publication, not all pre-registry side effects.
- Do not make permission cleanup transactional with client remap/delete, change the locked-store stale-permission exception, or turn its best-effort warning into a failed already-committed response.
- Do not change store lock/unlock/rotation, retained generations, resource data, policy modes/writable behavior, F-014 reference checks, runtime finalize/writeback, or store data-directory deletion.
- Do not change admin error codes/messages or resolve internal error-detail exposure (F-049). Do not alter OpenAPI security/status semantics or generated JSON (F-016), daemon request serialization (F-017), frontend credential reload guidance (F-018), or key/certificate pair installation atomicity (F-019).
- Do not add production fault-injection hooks, dependencies, storage formats, public DTO fields, serialization tags, routes, SPA behavior/source, manifests/locks, NixOS configuration/tests, privileged wrapper-chain changes, or binary/service topology.
- Do not absorb another finding, Opportunity, unresolved item, broad registry abstraction, migration, or severity transition.

## Acceptance criteria

### Failed persistence is not live publication

- Store update, store delete, and client update each persist/delete under the corresponding existing registry write lock before publishing the candidate/removal live.
- Every injected pre-commit save/delete error returns the existing stable 500 code and leaves the complete prior live record/entry unchanged. Store state/backend/rotation-lock identity and all runtime-client authorization fields remain unchanged.
- After the fault path is restored, a fresh registry load is exactly equal to the pre-request persisted state. No failed client remap permission cascade ran.
- No success audit is emitted for a persistence failure; existing error and audit vocabulary otherwise remains unchanged.

### Success and compatibility

- Retrying each operation after restoration succeeds and leaves live state exactly equal to a fresh disk reload of the requested state.
- Successful runtime-client mutation takes effect for subsequent runtime authentication without restart. With the replacement kept active, credential rotation accepts the replacement and rejects the superseded certificate; a later successful disable rejects the replacement, and re-enable accepts it again.
- Successful client remap/delete retains the existing post-commit best-effort permission cleanup behavior. Store/client create and client delete retain their current failure-safe and success behavior.
- Existing validation/error precedence, duplicate checks, immediate list views, store locked/unlocked state, data backends, action summaries, one-time encrypted key response, and ordinary runtime/admin behavior remain unchanged.

### Real boundary, documentation, and scope

- A real authenticated admin-HTTPS/runtime-mTLS matrix proves a returned registry-persistence 500 does not change live certificate acceptance, enablement, store mapping, auto-create authority, store metadata, or store availability; restored disk reload is equal and the same service/session recovers.
- Current backend/security/testing docs state the persist-before-publish running-process guarantee and explicitly retain cross-registry concurrency, best-effort cascade, repair, and power-loss/fsync limits.
- F-015 is resolved. F-016–F-019, F-049, O-003/O-004, locked-store stale-permission coordination, pre-existing repair, and every other finding/Opportunity remain unchanged.
- No OpenAPI JSON, public DTO, SPA source/schema, dependency/manifest/lock, Nix, privileged-chain, Python-bytecode, browser-output, or source path outside the approved implementation/ticket/handover allowlists changes.

## Security boundary

- Protected asset: the running backend's runtime-client authentication/enablement/store-mapping authority and the availability/identity of its live store registry, together with equivalence to the registry state a restart will load.
- Actor/fault: an authenticated full administrator or buggy admin client submits a valid registry mutation while an independent backend-host filesystem failure prevents the atomic record save/delete. The administrator remains trusted; the persistence failure must not make the 500 response lie about authorization or availability state.
- Boundary direction: authenticated admin HTTPS request -> candidate validation under the registry mutation lock -> atomic registry-file save/delete -> live registry publication -> subsequent runtime/admin request resolution.
- Invariant: if registry persistence reports failure, no candidate record or removal is published to the running registry; if persistence succeeds, the exact committed mutation becomes live before the registry lock is released.

## Validation

Run resource-intensive checks sequentially with `CARGO_BUILD_JOBS=2`.

- Preserve and hash the clean-HEAD red-before and baseline artifacts recorded above.
- `sha256sum /tmp/underwrap-codebase-review-2026-07-18-WJidbr/1140-{red-before.patch,red-before.log,baseline.log}`
- `cargo test -p underwrap-backend f015`
- `cargo test -p underwrap-backend admin_api::stores::crud`
- `cargo test -p underwrap-backend admin_api::client_instances`
- `cargo test -p underwrap-backend`
- `cargo test -p integration-tests registry_failure_atomicity`
- `cargo test -p integration-tests`
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- Run the OpenAPI drift test and confirm `crates/underwrap-backend/openapi.json` is unchanged.
- `cd web && npm test && npm run build`; confirm generated `web/src/api/schema.ts` remains uncommitted local output.
- Static checks for persistence-before-publication ordering, complete candidate validation under the same-registry lock, absence of reverse store/client lock nesting, post-commit-only cascade, exact live/reloaded failure and retry matrices, docs synchronization, exact staged allowlist, credential/secret hygiene, and no generated/Python/browser artifacts.
- No VM test is required for this backend admin/runtime registry boundary. If implementation crosses into NixOS, identities, namespaces, capabilities, mounts, peer authentication, or the privileged wrapper chain, stop because that is outside approved scope.
- `nix flake check`; preserve exact output if the known clean-baseline LLVM 21 `llvm-profdata merge` SIGILL recurs after all coverage tests pass.

## Dependencies

- Completed F-004/F-005 provide the real HTTPS/mTLS transport used by the boundary regression.
- Completed F-014 makes the currently published runtime-client registry authoritative for permission reference checks; a failed F-015 update must therefore leave that published authority and its persisted reload equivalent.
- The user-collection `mutate_collection` path is the local persist-under-lock pattern; it has no separate live registry and is not changed.
- F-016–F-019, F-049, O-003/O-004, locked-store stale-permission coordination, repair/migration, and all other non-goals remain separately queued.
- No unresolved implementation dependency. Mandatory repository-wide and ticket-local behavior-preserving cleanup discovery begins only after this exact task is approved. No cleanup unit is pre-approved.
