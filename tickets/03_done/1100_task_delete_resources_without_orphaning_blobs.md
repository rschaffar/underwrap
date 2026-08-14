# Delete resources without orphaning content blobs

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-011
Source themes: C-001

## Outcome

A successful resource deletion removes both the `ResourceRecord` and its empty `resources/<resource_id>` blob. Deletion refuses before mutation whenever any typed resource content, wrapper/resource pair, or permission row remains. In particular, libsecret-only content can no longer disappear from the admin catalog while its secret-bearing blob remains at rest.

The complete typed `ResourceBlob` is the content authority. The flat env/files projection is not a safe lifecycle view because it intentionally omits the structured libsecret section. The delete blocker response therefore uses typed content references rather than calling every blocker a flat “secret”. This is a clean first-release admin response correction: the old `secret_count` and `sample_secret_ids` fields are removed rather than retained as compatibility aliases.

When all blockers are absent, the backend requests deletion of the active empty-blob storage key before persisting removal of the catalog record. A returned blob-delete error prevents any catalog write. If the later catalog save returns an error, the request also fails. Storage-trait errors do not prove that an attempted operation had no externally visible effect, so operators re-read state before a serialized retry; an explicitly missing blob resolves as typed-empty and never needs reconstruction.

## Scope

- Add an identifier-only `ResourceContentReference` type for delete-blocker samples. Define it exactly as an internally tagged enum (`#[serde(tag = "kind", rename_all = "snake_case")]`) with named fields and matching `utoipa` discrimination so OpenAPI/code generation produces a usable union with these variants:
  - `env { name }`
  - `file { path }`
  - `libsecret_alias { name }`
  - `libsecret_collection { collection_id }`
  - `libsecret_item { collection_id, item_id }`
- Give `ResourceBlob` one authoritative deterministic content enumeration. Count each env entry, file entry, alias mapping, collection (including an empty collection), and item once. Traverse env keys, file keys, aliases, then collections and each collection’s items in `BTreeMap` order. Do not include values, labels, alias targets, attributes, content types, timestamps, or secret bytes.
- Replace `AdminResourceDeleteBlockers.secret_count` / `sample_secret_ids` with `content_entry_count` / `sample_content`. The total is the complete pre-sampling count, saturated to `u32`; the sample contains at most `ENTITY_DEPENDENT_SAMPLE_LIMIT` (currently five) typed references. Remove the old fields with no compatibility branch. Preserve the existing pair and permission blocker fields.
- Replace the flat-only resource-content scan with the complete typed enumeration. Malformed, oversized, unsupported-version, unreadable, or undecryptable resource blobs fail the request before any mutation; only an explicit storage-key-not-found result is typed-empty.
- Keep blocker aggregation: one soft refusal reports complete content, pair, and permission counts/samples together. A blocked request changes neither blob nor catalog and emits only the existing safe blocked audit event.
- On an unblocked delete, call `delete_resource_blob` before removing/saving the `ResourceRecord`. `Ok(false)` is valid for a never-materialized/missing empty blob; a deletion error is fatal and prevents the catalog write. Save the catalog only after blob deletion succeeds, and emit success/a return body only after the catalog save succeeds.
- Add failure-order evidence with an explicit fail-before-operation test backend and call trace: a returned blob-delete error prevents the catalog write; a catalog-write error after successful empty-blob deletion returns an error and triggers no reconstruction write. Separately re-read resulting state rather than treating an error as proof that an attempted storage operation was invisible.
- Prove that the active blob key becomes absent for both plain and encrypted `StorageBackend` implementations, plus real authenticated admin HTTP behavior and persisted-store state. Do not claim secure media erasure or deletion from retained encrypted generations.
- Regenerate and commit `crates/underwrap-backend/openapi.json`. Update the SPA delete dialog to render every typed content-reference variant, render existing pair and permission-row samples, and use “stored resource content” and “permission rows” rather than the current flat “secrets”/“grants” wording. Extend its Vitest and the existing Playwright resource-deletion flow for libsecret-only refusal and eventual successful cleanup.
- Synchronize present-state resource lifecycle text in `docs/backend.md`, `docs/admin-ui.md`, `docs/security.md`, and `docs/testing.md`.
- Preserve the deterministic clean-HEAD red-before artifacts outside the repository:
  - patch `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1100-red-before.patch`, SHA-256 `8cd8a956c63554f98d30cab2346a1949441d3e9924d35261cb2aaa4c77a3ad1c`;
  - log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1100-red-before.log`, SHA-256 `cdabc48a020b009b710e3180c5b5dd8cf0ee183be4ca11ffe4d0486b1ed43859`.
  Both desired tests fail at clean HEAD `972c5b3b82d7f54dcc502289e40dc6d1f0905c4c`: deletion returns `ok=true`, removes the catalog record, and retains the blob for both libsecret-only and persisted-empty cases.

Expected implementation paths are limited to the coherent protocol/backend/store/UI/documentation boundary:

- `crates/underwrap-protocol/src/resource_blob.rs`
- `crates/underwrap-protocol/src/resource_blob/tests.rs`
- `crates/underwrap-protocol/src/service/admin.rs`
- `crates/underwrap-backend-store/src/resource_blob/tests.rs`
- `crates/underwrap-backend/src/storage/documents.rs`
- `crates/underwrap-backend/src/storage/documents/tests.rs`
- `crates/underwrap-backend/src/storage/secret_entries.rs`
- `crates/underwrap-backend/src/admin_api/resources.rs`
- `crates/underwrap-backend/src/admin_api/resources/tests.rs`
- `crates/underwrap-backend/src/test_support.rs` only if the existing failure backend needs a narrowly scoped call-order capability
- `crates/underwrap-backend/openapi.json`
- `crates/integration-tests/src/tests/service_transport.rs`
- `web/src/pages/secrets/dialogs/delete-resource.tsx`
- `web/src/pages/secrets/dialogs/delete-resource.test.tsx`
- `web/e2e/secrets.spec.ts`
- `docs/backend.md`
- `docs/admin-ui.md`
- `docs/security.md`
- `docs/testing.md`

Any approved behavior-preserving cleanup has its own separately reviewed scope and commit before the finding fix.

## Non-goals

- Do not add force-delete, cascading deletion, tombstones, background garbage collection, orphan discovery/repair, migration logic, or compatibility fields for the unshipped blocker response.
- Do not permit deletion while any typed content, pair, or permission remains. Operators clear each blocker through its existing endpoint first.
- Do not validate, normalize, resolve, repair, or discard malformed/dangling libsecret identifiers while enumerating content. Stored aliases, collections, and items count exactly as represented. F-045 remains separate.
- Do not add absent-resource/reference validation to admin secret/libsecret/permission endpoints, reject post-delete writes, or repair pre-existing orphan/dangling state. F-014 remains separate.
- Do not redesign publication/writeback failure atomicity, registry publication, or other mutation error behavior. F-012/F-015 and O-011 remain separate.
- Do not add transactions, revisions, compare-and-swap, per-resource/store mutation locks, or a stronger concurrency claim. Existing same-object load/modify/save behavior remains last-writer-wins; deployments must serialize every relevant mutation, including runtime writeback and admin content, pair, permission, and deletion changes. O-004 remains separate.
- Do not claim that a returned error proves an arbitrary storage operation had no externally visible effect, or absorb F-049 error-redaction work. The selected ordering guarantees no success response before both required operations report success.
- Do not change the canonical persisted `ResourceBlob` schema, size limits, runtime wire protocol, permission policy, wrapper behavior, storage encryption, rotation retention, dependencies, NixOS module, target identities, capabilities, namespaces, services, or the `underwrap-exec -> underwrap-ns` chain.
- Do not absorb any medium/low finding, Opportunity, unresolved item, or the high-to-medium transition.

## Acceptance criteria

### Complete typed content blockers

- `ResourceBlob` content enumeration returns no references for a typed-empty blob and exact typed references for env, files, libsecret aliases, empty/nonempty collections, and items in the specified deterministic order.
- `content_entry_count` equals `env.len + files.len + aliases.len + sum(1 + items.len)` over collections before saturation/sampling. A collection and each of its items are separate content entries.
- Samples copy only raw identifier fields into structured JSON and rely on JSON/UI escaping; no pseudo-path parser/escaping grammar is introduced. They never copy value-bearing fields, labels, alias targets, attributes, content types, or timestamps. Because an untrusted identifier can itself contain sensitive text, samples are never copied into audit or log fields and are not described as inherently secret-free.
- Malformed or dangling aliases still count as alias content without being validated or dereferenced. A blob decode/size/version failure returns an internal error before mutation.
- The admin wire/OpenAPI contract contains only `content_entry_count` and `sample_content` for resource content blockers; `secret_count` and `sample_secret_ids` are absent. Pair and permission blocker behavior is unchanged.
- Mixed content/reference state returns one `200 { ok: false, blocked_by: ... }` with exact content, pair, and permission totals and capped deterministic samples. Catalog/blob bytes remain unchanged and the blocked audit contains identifiers/result only.

### Empty-blob and failure ordering

- A resource with a missing blob and no references deletes successfully.
- A resource with a persisted typed-empty blob and no references deletes successfully and leaves neither the catalog record nor `resources/<resource_id>` storage key.
- For plain and encrypted backends, the active empty-blob key becomes absent (`read` returns `None` and listing omits it); repeated lower-level deletion reports `false` without creating state. No secure-erasure or retained-generation claim is made.
- Blob read/decode failures cause no mutation. A returned blob-delete error causes no later catalog-write call. Neither case emits a success response or success audit.
- If blob deletion reports success but the resources-document save returns an error, the response is an error and no reconstruction write is attempted. Tests re-read actual state; docs require re-read/reconciliation before retry rather than promising that a returned storage error proves whether the attempted catalog write became visible.
- The committed successful path is ordered: load/validate all content and references, delete the empty/missing blob, remove/save the resource record, then audit/respond success.
- The ticket makes no CAS/transaction guarantee. Documentation explicitly retains last-writer-wins and requires serialization across all relevant runtime/admin mutations, and assigns absent/post-delete writes to F-014.

### Real route, UI, docs, and regression

- A real authenticated HTTPS admin flow creates libsecret-only content, receives a typed content blocker on resource deletion, preserves the catalog/blob, clears the structured content, then deletes the resource and proves the active blob key is absent.
- SPA blocker UI renders every content-reference variant and existing pair/permission samples without copying value-bearing fields, uses “stored resource content”/“permission rows” wording, relies on React escaping for identifiers, stays open on `ok=false`, and closes/invalidates views only on `ok=true`.
- The existing Playwright resource-delete flow proves a libsecret blocker is visible before cleanup and that final deletion removes the resource block.
- Committed OpenAPI exactly matches the live `ApiDoc`; generated `web/src/api/schema.ts`, `web/dist`, Playwright output, Python bytecode, and other generated artifacts are not staged.
- Present-state docs describe complete typed blockers, active empty-blob-key removal (not secure media erasure), failure ordering and error-state reconciliation, identifier-only samples, and the explicit all-mutation serialization/F-014 limits without ticket/history prose.
- No source or artifact lies outside the approved implementation, ticket, and handover allowlists; no other F-id is changed.

## Security boundary

- Protected asset: every persisted resource blob and its secret-bearing env, files, and libsecret content, together with the operator’s ability to account for and intentionally remove that material.
- Attacker/failure source: no new untrusted principal gains authority; the failure is an authenticated operator deletion crossing an incomplete classifier or storage error, after which retained material can become unreachable and survive at rest.
- Boundary direction: authenticated admin catalog delete -> complete typed resource-content check -> empty blob deletion -> resource catalog persistence.
- Invariant: when all relevant mutations are serialized, `ok: true` is emitted only after every typed content/reference blocker is absent, active blob-key deletion reports success or absence, and catalog persistence reports success. A blocked request issues no mutation. Any storage error reports failure and prevents later handler operations, but callers must re-read state because an error does not prove that the attempted operation was externally invisible.

## Validation

Run resource-intensive checks sequentially with `CARGO_BUILD_JOBS=2`.

- Preserve the clean-HEAD red-before patch/log and exact SHA-256 values recorded above.
- `cargo test -p underwrap-protocol resource_blob`
- `cargo test -p underwrap-backend-store resource_blob`
- `cargo test -p underwrap-backend admin_resource_delete`
- `cargo test -p integration-tests admin_resource_delete`
- Run `cargo test -p underwrap-backend --test openapi_export` once to regenerate an expected drift, inspect `openapi.json`, then rerun it and require PASS with zero further diff.
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cd web && npm test && npm run build`
- `nix develop -c bash -lc 'cd web && npm run test:e2e -- secrets.spec.ts'`
- `nix flake check`; if the known clean-baseline LLVM 21 `llvm-profdata merge` SIGILL recurs after coverage tests pass, preserve the exact output and do not misattribute it to F-011.
- Static checks for exact tagged typed-reference schema/order/count, absence of old blocker fields, exclusion of value-bearing fields and samples from logs/audit, delete-before-catalog ordering, all referenced F-id exclusions, OpenAPI/schema drift, exact staged allowlist, no generated/Python artifacts, `git diff --check`, and staged credential-pattern hygiene.

No standalone NixOS VM rerun is required because this ticket changes no NixOS or privileged frontend boundary; `nix flake check` remains the full repository gate.

## Dependencies

- Completed F-001 preserves untouched typed sections during flat mutations; completed F-003 preserves encrypted generations; completed F-008/F-009 provide strict complete-resource publication/size handling. This ticket relies on their current contracts and does not revise them.
- F-014 remains the separately queued absent-reference/orphan-creation correction. F-012/F-015/F-045/F-049 and O-004/O-011 remain separate as stated above.
- No unresolved implementation dependency. Mandatory repository-wide and ticket-local behavior-preserving cleanup discovery begins only after this exact task is approved. No cleanup unit is pre-approved.
