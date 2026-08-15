# Reject missing admin entity references

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-014
Source themes: C-004

## Outcome

Explicit resource-addressed flat-secret and libsecret content APIs act only on an existing `ResourceRecord` in the selected store. A syntactically valid content request naming a missing resource receives HTTP 404 with the existing stable `not_found` error code before the backend loads, creates, changes, or deletes `resources/<resource_id>`. Ordinary pair creation and wrapper-permission upsert likewise reject missing references; cleanup-oriented permission/pair list and delete routes remain able to surface and remove stale rows.

A wrapper permission can be created or updated only when its runtime client instance exists and is currently assigned to the selected store, its resource exists in that store, and its exact `(wrapper, resource_id)` pair exists there. Invalid references receive the same canonical 404 envelope before the policy document changes, so they cannot become latent authorization if matching entities appear later.

This closes deterministic sequential admin-request paths that create orphan resource blobs or dangling permission rows. It does not claim cross-request transactions, compare-and-swap, concurrent runtime/admin serialization, repair of pre-existing inconsistent data, or cleanup of the separately documented locked-store stale-permission case.

## Scope

- Centralize the selected store's entity-reference checks so flat-secret, libsecret, pair, and permission handlers do not each invent different existence rules:
  - resource existence means an exact `ResourcesDocument::find_by_id(resource_id)` match in that store;
  - client existence for a permission means an exact record in the currently published in-memory client-instance registry whose `current_store_id` equals the selected store id; status does not affect reference validity, so disabled clients remain valid references; F-015 separately owns equivalence between published and persisted registry state after save failure;
  - pair existence means an exact `(wrapper, resource_id)` row in that store's `PairsDocument`;
  - missing entities use HTTP 404, `ServiceErrorResponse.code == "not_found"`, and generic identifier-value-free messages `unknown resource`, `unknown client instance`, or `unknown pair`.
- Preserve each handler's existing syntax/body-validation precedence. Resolve the store before the new reference gate, except where that handler already validates or decodes input earlier. The new resource gate must precede addressed resource-blob access; the permission reference gates must precede policy-document mutation. Do not reorder subordinate failures such as optional rename material decoding versus missing source, collection/item id validation versus missing child, or existing slot/key validation. For permission upsert, retain non-empty tuple validation first, then resolve the store, check client-in-this-store, resource, and exact pair in that stable order, and stop on the first missing reference.
- Apply resource validation to every resource-addressed flat-secret route:
  - filtered `GET /v1/admin/stores/{id}/secrets?resource_id=...`;
  - `GET`, `PUT`, and `DELETE /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}`;
  - `POST /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}/rename`.
  Missing resources return 404 before a resource-blob read/write/delete. Unfiltered listing remains a store-wide diagnostic view and is not turned into an implicit repair pass.
- Apply the same resource validation through the shared read/mutation seams used by every structured libsecret route:
  - tree read and collection create/delete;
  - item create/delete and item-secret read;
  - alias put/delete.
  An existing resource with no active blob still has the documented empty `ResourceBlob` starting state; only a missing catalog entity is rejected.
- Harden wrapper-permission upsert without blocking cleanup:
  - reject a missing client instance or a client currently assigned to another store;
  - reject a missing resource or missing exact pair;
  - leave the policy document byte-for-byte unchanged on every rejection;
  - retain ordinary create/update behavior, permission modes, writable flag, reason, sorting, audit behavior, and disabled-client support for valid references;
  - keep permission list and delete capable of surfacing and removing stale rows rather than making them unmanageable.
- Route ordinary pair creation's existing missing-resource check through the canonical resource-reference contract. Change that one case from HTTP 400 code `unknown_resource` to HTTP 404 code `not_found`; preserve pair syntax, duplicate conflict, provenance, sorting, and successful behavior. `pairs/with-new-resource` creates its own resource and is unchanged.
- Add deterministic negative and compatibility evidence:
  - every flat resource-addressed handler rejects a missing resource with the exact 404/code and causes no resource-blob operation or catalog/policy change;
  - all eight libsecret operations reject a missing parent resource with the exact 404/code; representative read and mutation cases additionally use operation tracing to prove the shared seams perform no resource-blob access, so an empty orphan blob is not treated as an entity;
  - permission upsert independently covers missing client, wrong-store client, missing resource, missing pair, existing disabled client, valid create, and valid update, with unchanged policy on rejection;
  - pair creation covers canonical missing-resource 404 and unchanged pairs;
  - existing-resource missing-key/child and ordinary idempotent-delete behavior remains covered;
  - a real authenticated admin HTTPS regression exercises representative flat-secret, libsecret, permission, and pair rejection, verifies the exact error envelope and unchanged aggregate/resource/pair/policy views, then proves valid requests still work through the same service/session.
- Keep test servers, clients, backend processes, temporary stores, and helpers bounded and deterministically stopped on every path. Apply any hard timeout in the parent process; do not cancel and drop an in-progress asynchronous cleanup future.
- Synchronize current-state behavior in `docs/backend.md`, `docs/security.md`, and `docs/testing.md`.
- Update only F-014-relevant handler response descriptions and the committed generated `crates/underwrap-backend/openapi.json`; run the web schema generator/build as validation, but do not commit local `web/src/api/schema.ts` build output.
- Preserve these clean-HEAD revalidation artifacts outside the repository:
  - desired red-before patch `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1130-red-before.patch`, SHA-256 `d0183df426e5423746405a365115f265858b68c807b20f3038044959b97188b3`;
  - desired red-before log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1130-red-before.log`, SHA-256 `c684e11ccec6874ff51b644d8f7759194d66f01f1f1908a2dcd51aeb1d228019`;
  - clean baseline log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1130-baseline.log`, SHA-256 `d42e13b34288f7ff408789aa3a4f8c1a6ba1b342d5408fff6076074bdfc0a0bd`.
  At clean HEAD `e55bd81796769238477c07795e12773f2cc1478a`, the three desired tests fail with status 101 and exact observations `(200, blob exists)` for flat-secret PUT, `(200, blob exists)` for libsecret collection POST, and `(200, one permission row)` for permission PUT; each expects `(404, no effect)`. The unmodified backend baseline passes 242 library tests, three binary tests, and OpenAPI drift.

Expected implementation paths are limited to:

- `crates/underwrap-backend/src/admin_api/mod.rs`
- `crates/underwrap-backend/src/admin_api/references.rs`
- `crates/underwrap-backend/src/admin_api/references/tests.rs`
- `crates/underwrap-backend/src/admin_api/secrets.rs`
- `crates/underwrap-backend/src/admin_api/secrets/tests.rs`
- `crates/underwrap-backend/src/admin_api/libsecret.rs`
- `crates/underwrap-backend/src/admin_api/libsecret/tests.rs`
- `crates/underwrap-backend/src/admin_api/policy.rs`
- `crates/underwrap-backend/src/admin_api/policy/tests.rs`
- `crates/underwrap-backend/src/admin_api/pairs.rs`
- `crates/underwrap-backend/src/admin_api/pairs/tests.rs`
- `crates/integration-tests/src/tests/admin_reference_integrity.rs`
- `crates/integration-tests/src/tests/mod.rs`
- only the existing integration harness path(s) strictly needed by that matrix
- `crates/underwrap-backend/openapi.json`
- `docs/backend.md`
- `docs/security.md`
- `docs/testing.md`

A narrower path set is preferred if it meets every acceptance criterion. Any approved behavior-preserving cleanup has its own separately reviewed scope and commit before the finding fix.

## Non-goals

- Do not change an existing resource's subordinate missing-content semantics: flat secret reads may still return `material_b64: null`; flat and libsecret idempotent deletes retain their `existed: false` responses; existing-resource missing collection/item/key errors otherwise remain as currently defined. F-016 owns broader status/annotation reconciliation.
- Do not scan, hide, delete, migrate, garbage-collect, synthesize catalog entries for, or otherwise repair pre-existing orphan blobs, dangling pairs, or stale permission rows. Unfiltered aggregate/list views remain diagnostic and permission delete remains able to remove stale rows.
- Do not change client-instance reassignment/deletion while a prior store is locked, add automatic cleanup on unlock, or alter the documented audit-visible stale-permission exception. That lifecycle coordination is not silently folded into a direct upsert-reference fix.
- Do not add cross-document transactions, rollback, CAS/revisions, per-store mutation locks, or serialization across concurrent admin/runtime registration, invocation, finalize, resource deletion, pair deletion, or content writes. Reference checks and subsequent blob/pairs/policy operations remain separate reads/writes: concurrent deletion, reassignment, registration, or mutation can invalidate a successful check before persistence. Preserve the documented last-writer-wins limitation and guarantee only serialized/sequential requests; O-004 and later coordination work remain separate.
- Do not change registry live/publication failure ordering or persistence fault behavior (F-015).
- Do not fix OpenAPI session+CSRF OR/AND semantics, global 423-versus-503 drift, omitted lifecycle branches, or unrelated annotations (F-016). Generated OpenAPI changes are restricted to the F-014 resource/reference 404 contract.
- Do not change target-facing Secret Service alias validation or persistence, rejected-candidate handling, item mutation timestamps, or wrong-typed properties (F-039/F-045/F-046). Admin alias targets already require an existing collection; this task adds only the missing parent-resource gate.
- Do not change `ResourceBlob` schema, resource-size limits, resource-delete blocker/deletion ordering, active-key-versus-secure-erasure claims, secret value handling, runtime policy re-evaluation, permission mode/writable semantics, or store encryption/rotation.
- Do not change public protocol DTO field types, serialization tags, SPA workflows/components, local generated TypeScript output, dependencies, Cargo/NPM manifests or locks, NixOS configuration/tests, privileged wrapper chain, or binary/service topology.
- Do not absorb another finding, Opportunity, migration, repair command, broad refactor, severity transition, or assertion weakening.

## Acceptance criteria

### Resource-addressed admin integrity

- Every explicit resource-addressed flat-secret and libsecret request first proves that exact `ResourceRecord` exists in the selected store before any active resource-blob read/write/delete.
- Missing resources return HTTP 404 with `ServiceErrorResponse.code == "not_found"` and message `unknown resource`; no request creates an empty or populated `resources/<resource_id>` key.
- Filtered flat listing rejects a missing resource; unfiltered listing remains unchanged.
- Existing resources with no blob retain a valid empty starting state. Existing-resource missing key/collection/item/alias behavior and idempotent delete responses remain unchanged.
- Unit operation tracing and persisted-state comparisons prove no resource blob, resources document, pairs document, policy document, or client registry changes on rejected requests.

### Permission and pair references

- Permission upsert returns the canonical 404 before policy mutation when the client is absent, is assigned to another store, the resource is absent, or the exact pair is absent. The stable check order is client-in-store, resource, then pair.
- A disabled client assigned to the store is a valid reference. Valid permission create/update retains mode, writable, reason, sorting, audit, and persistence behavior.
- Permission list/delete continue to expose and remove stale rows.
- Ordinary pair creation returns canonical HTTP 404/code `not_found` for a missing resource and leaves the pairs document unchanged; valid and duplicate-pair behavior remains unchanged.

### Real boundary, contract, and scope

- A real authenticated admin HTTPS matrix observes exact 404 envelopes and unchanged state for representative flat-secret, libsecret, permission, and pair requests, then succeeds on valid requests through the same live service/session.
- OpenAPI handler annotations and committed generated JSON accurately describe the F-014 resource/reference 404 behavior without changing F-016's separately queued security or lifecycle semantics.
- Current backend/security/testing docs state the sequential entity-reference rules and their concurrency/repair limits without claiming transactionality.
- F-014 is resolved. F-015/F-016, F-039/F-045/F-046, O-004, the locked-store stale-permission lifecycle, pre-existing-data repair, and every other finding/Opportunity remain unchanged.
- No Cargo/NPM manifest or lock, public protocol DTO, committed SPA source/schema output, Nix, privileged-chain, dependency, Python-bytecode, browser-output, or source path outside the approved implementation/ticket/handover allowlists changes.

## Security boundary

- Protected asset: the selected store's declared entity graph—resource catalog to active content blob, client/store assignment to exact wrapper/resource pair, and those references to persisted permission rows.
- Actor: an authenticated full administrator or buggy admin client. The administrator is trusted to choose valid policy/content, but request paths and identifiers can still be stale or erroneous; the API must not persist a graph that contradicts its own catalog and joined views.
- Boundary direction: authenticated admin HTTPS path/body -> syntax and selected-store resolution -> centralized entity-reference validation -> resource blob or pairs/policy persistence.
- Invariant: a sequential admin request cannot create content for a missing resource or a permission whose client/store/resource/pair references do not all exist. Rejection is a stable value-free 404 and has no persistence effect.

## Validation

Run resource-intensive checks sequentially with `CARGO_BUILD_JOBS=2`.

- Preserve and hash the clean-HEAD red-before and baseline artifacts recorded above.
- `sha256sum /tmp/underwrap-codebase-review-2026-07-18-WJidbr/1130-{red-before.patch,red-before.log,baseline.log}`
- `cargo test -p underwrap-backend admin_reference`
- Focused flat-secret, libsecret, policy, and pair sibling tests.
- `cargo test -p underwrap-backend`
- `cargo test -p integration-tests admin_reference_integrity`
- `cargo test -p integration-tests`
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- Regenerate and inspect `crates/underwrap-backend/openapi.json`; run the OpenAPI drift test.
- `cd web && npm test && npm run build`; confirm `web/src/api/schema.ts` remains uncommitted local output.
- Static checks for centralized catalog/client/pair validation, validation-before-blob/policy ordering, canonical 404/code/message, unchanged subordinate semantics, exact no-effect matrices, docs/OpenAPI synchronization, exact staged allowlist, credential hygiene, and no generated/Python/browser artifacts.
- No VM test is required for this backend admin/API boundary under the repository validation matrix. If implementation crosses into NixOS, identities, namespaces, capabilities, mounts, peer authentication, or the privileged wrapper chain, stop because that is outside approved scope.
- `nix flake check`; preserve exact output for the known clean-baseline LLVM 21 `llvm-profdata merge` SIGILL after tests and disclose any unrelated F-007/F-009 VM instability without weakening assertions or attributing it to F-014.

## Dependencies

- Completed F-011 makes the typed resource blob authoritative for deletion blockers and removes the active empty blob before the catalog row; F-014 closes the separately retained absent/post-delete sequential write paths and must not weaken F-011 ordering or claims.
- Completed F-013's stable pre-storage runtime registration validation and ordinary valid auto-create behavior remain unchanged.
- F-015/F-016, F-039/F-045/F-046, O-004, locked-store stale-permission lifecycle coordination, repair/migration, and all other non-goals remain separately queued.
- No unresolved implementation dependency. Mandatory repository-wide and ticket-local behavior-preserving cleanup discovery begins only after this exact task is approved. No cleanup unit is pre-approved.
