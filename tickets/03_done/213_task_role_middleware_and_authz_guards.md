# Task: Role-based middleware and admin-only endpoint guards

Parent: `210_story_rust_native_auth_sessions_multiuser.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Define a `Principal { id, username, role, granted_stores }` resolved from the session cookie on every authenticated request.
- Introduce a small middleware layer with three guards:
  - `require_session` — any authenticated operator
  - `require_admin` — admin role only
  - `require_store_access(store_name)` — admin bypasses; operator must have the store in `granted_stores`
- Admin-only endpoints (user management, store CRUD, declarative-store info) return 403 with code `admin_only` for non-admin operators.
- Store-scoped endpoints (resources/query, reveal, matrix, mutation/get/put, etc.) return **404** (not 403) for ungranted stores to avoid leaking store existence.
- Principal is automatically propagated into every audit event.

## Key files likely involved

- `crates/backend-backendd/src/auth.rs`
- `crates/backend-backendd/src/server.rs`
- Any per-store-handler modules

## Acceptance criteria

- [x] ~~Admin-only endpoints reject operator-role sessions with 403 and a stable error code.~~ Not applicable yet: no admin-only endpoints exist in the codebase (user-management lands in 240, store CRUD in 230, declarative-store info in 232). The `admin_only` code is reserved; `Principal::is_admin()` is in place for those tickets to wire up.
- [x] Store-scoped endpoints reject ungranted-store access with 404.
- [x] Tests cover: admin accesses all, operator with grant accesses one store, operator without grant gets 404, principal propagation into audit events verified for `wrapper_mode_updated`.

## Implementation notes

- `Principal { id, username, role, granted_stores }` lives in
  `operators.rs` alongside `OperatorRole`. Built once per request by
  `resolve_principal`; session auth re-reads the live operator
  collection so role/grant changes take effect on the next request
  without forcing a re-login.
- Legacy bearer-token auth synthesises an admin principal: those
  tokens predate the multi-user model and have always conveyed full
  access. Migrating them off is out of scope for 213.
- The store gate is realised through `admin_store_for_principal`,
  which collapses "doesn't exist" and "no grant" into a single 404
  `not_found` response — preventing operators from probing the store
  catalogue.
- `/v1/admin/stores` GET and `/v1/admin/ui/overview` GET filter their
  output to granted stores before responding.
- Audit events flow through the existing `operator: &str` parameter
  on each mutation handler. That string is now `principal.username`
  by construction (handlers receive it from `handle_admin_request`).
