# Task: Extract `runtime_state`, `admin_api/csrf`, `admin_api/audit`

Parent: `700_story_backend_modularization.md`

## Scope

Pull the cross-cutting glue that every admin handler depends on into
its own modules: the shared in-memory state, the session/CSRF gate,
the principal resolver, the audit-log helper, and the cookie
refresher. Doing this before splitting the handlers themselves means
each handler module can just `use crate::admin_api::csrf::*;` without
circular dependencies.

### `crates/backend-backendd/src/runtime_state.rs` (new)

Move verbatim from `server.rs`:

- `AdminRuntimeState` struct (242-302). Field visibility stays
  `pub(crate)` so the submodules under `admin_api/` and `runtime_api/`
  can read them. (Do **not** split the struct in this ticket — see
  epic.)

### `crates/backend-backendd/src/admin_api/mod.rs` (new, skeleton)

Declares the `audit` and `csrf` submodules. `handle_admin_request`
stays in `server.rs` for now; ticket 810 finishes the dispatcher
refactor.

### `crates/backend-backendd/src/admin_api/csrf.rs` (new)

Move verbatim from `server.rs`:

- `AdminAuth` struct + `impl` (2151-2163)
- `AdminAuthError` enum (2169-2172)
- `authenticate_admin` (2175-2201)
- `check_csrf` (2206-2245)
- `resolve_principal` (2121-2148)
- `attach_csrf_cookie` (6208-6228)

### `crates/backend-backendd/src/admin_api/audit.rs` (new)

Move verbatim from `server.rs`:

- `log_admin_event` (304-323)

(The per-subdomain audit emitters — `write_admin_store_audit`,
`write_admin_user_audit`, `write_admin_permission_audit`,
`audit_bootstrap_attempt` — stay with their subdomain handlers and
move with them in their respective tickets.)

### Call-site updates

`server.rs` now imports these from `crate::runtime_state::*` and
`crate::admin_api::{csrf, audit}::*`. No semantic changes.

## Acceptance criteria

- [ ] `runtime_state.rs`, `admin_api/mod.rs`, `admin_api/csrf.rs`,
      `admin_api/audit.rs` exist with the listed contents.
- [ ] `server.rs` no longer defines any of the listed items.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.

## Out of scope

- Splitting `AdminRuntimeState` into multiple structs.
- Moving the public auth endpoints (`/v1/admin/auth/*`) — those go in
  ticket 760.
