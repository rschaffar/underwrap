# Task: Rename Rust protocol types and backend handlers

Parent: `440_story_rename_to_wrapper_permissions.md`

Land the source-of-truth rename: protocol types, methods, JSON shape,
backend handlers, admin endpoint paths, and the integration test
harness. SPA work is blocked on the regenerated `ts-rs` types this
task produces.

## Scope

### `crates/shared-protocol/src/policy.rs`

- Rename struct `AgentGrant` → `WrapperPermission`. Update every
  reference in this file (impls, doc comments, tests).
- Rename `PolicyDocument.grants` field → `permissions`. The serialized
  JSON key flips from `"grants"` to `"permissions"`. Greenfield: no
  migration; existing dev stores must be re-seeded after merge.
- Rename methods on `PolicyDocument`: `find_grant` →
  `find_permission`; `evaluate_grant`, `evaluate_grant_at`,
  `evaluate_grant_after_approval` → `evaluate_permission{,_at,_after_approval}`.
- Rename the free `decide(grant, …)` helper → `decide(permission, …)`.
- Rewrite the module-level doc comment (lines 1–17 today): "A grant is
  keyed on…" → "A wrapper permission is keyed on…"; replace "the
  matrix UI" with "the wrapper permissions UI".
- Update the formatted `PolicyDecision::Deny { reason }` strings
  ("grant for '…' is locked / expired") → "permission for …".
- Update all unit test fixtures (`doc_with_grant`, the multi-mode
  roundtrip test) to the new names.

### `crates/shared-protocol/src/service.rs`

- Rename `AdminAgentGrantsListRequest`, `AdminAgentGrantsListResponse`,
  `AdminAgentGrantUpsertRequest`, `AdminAgentGrantDeleteRequest` to
  `AdminWrapperPermissionsListRequest`, `…ListResponse`,
  `AdminWrapperPermissionUpsertRequest`, `AdminWrapperPermissionDeleteRequest`.
- Update the `===== Agent grant CRUD =====` section comment.

### Stable error codes

- Rename `duplicate_grant` → `duplicate_permission` wherever the code
  is emitted or asserted on (search `crates/` for the string).

### `crates/backend-backendd/src/server.rs`

- Rename `handle_admin_grants_list`, `handle_admin_grant_upsert`,
  `handle_admin_grant_delete` → `handle_admin_wrapper_permissions_*`.
- Update route registration: `/v1/admin/grants/{list,upsert,delete}`
  → `/v1/admin/wrapper-permissions/{list,upsert,delete}`.
- Update the two `===== Agent grant CRUD =====` section comments
  (lines ~4100 and ~5319 today).
- Update any audit event names that include "grant" in this context
  (`grant.upsert` → `wrapper_permission.upsert`, etc.).
- Update `crates/backend-backendd/tests/ts_export.rs` to list the new
  type names so `web/src/api/types/*.ts` regenerates cleanly. Confirm
  the four old `AgentGrant.ts` / `AdminAgentGrant*.ts` files no
  longer regenerate.

### Call sites

- `crates/shared-store/`, `crates/frontend-underwrapd/`: update every
  `AgentGrant` reference, `.grants` field access, and renamed-method
  call site (mechanical; compiler-driven).
- `crates/integration-tests/src/harness/mod.rs`: rename harness
  helpers that wrap grant CRUD.
- `crates/integration-tests/src/tests/grants.rs`: rename file to
  `wrapper_permissions.rs`; update test names. Re-export the new file
  from the tests `mod.rs`.

### Examples + configs

- `examples/config.yaml` and any other on-disk example referencing the
  old `grants:` key or endpoint paths.

## Verification

- [ ] `cargo fmt -- --check`
- [ ] `cargo clippy --all-targets --workspace -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `rg -i 'agent[- _]?grant' crates/ shared-protocol/ examples/`
      returns nothing outside of ssh-agent or `docs/agents.md`.

## Out of scope

- SPA changes — covered by 442.
- Doc rewrites — covered by 443. (But updating the ts_export test
  contents *is* part of this task so 442 has the regenerated TS to
  work with.)
- The `/wrappers` SPA route path — stays as-is.
