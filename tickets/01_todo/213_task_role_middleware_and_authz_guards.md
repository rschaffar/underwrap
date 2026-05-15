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

- [ ] Admin-only endpoints reject operator-role sessions with 403 and a stable error code.
- [ ] Store-scoped endpoints reject ungranted-store access with 404.
- [ ] Tests cover: admin accesses all, operator with grant accesses one store, operator without grant gets 404, principal propagation into audit events verified for at least one mutation per category.
