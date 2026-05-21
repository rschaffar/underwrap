# Task: Extract `admin_api/auth` and `admin_api/bootstrap`

Parent: `700_story_backend_modularization.md`

## Scope

Move the admin login surface (login / logout / whoami /
password-change) and the first-operator bootstrap flow into their own
modules. These two surfaces are the only `/v1/admin/*` paths the
existing `handle_admin_request` allows through *before* the
session-cookie check; keeping them together as the first admin
extraction makes the dispatcher's pre-auth section easy to read in
isolation.

### `crates/backend-backendd/src/admin_api/auth.rs` (new)

Move verbatim from `server.rs`:

- `LoginRequest` (2249-2252), `LoginResponse` (2255-2258),
  `WhoamiResponse` (2261-2270)
- `handle_admin_login` (2272-2387)
- `handle_admin_logout` (2611-2636)
- `handle_admin_whoami` (2638-2690)
- `handle_admin_password_change` (3044-3147)

### `crates/backend-backendd/src/admin_api/bootstrap.rs` (new)

Move verbatim from `server.rs`:

- `BootstrapRequest` (2389-2394)
- `BootstrapStatusResponse` (2396-2400)
- `BootstrapResponse` (2402-2408)
- `handle_admin_bootstrap_status` (2411-2427)
- `handle_admin_bootstrap` (2429-2592)
- `audit_bootstrap_attempt` (2594-2609)

### Call-site updates

`handle_admin_request` in `server.rs` updates its match arms to call
`crate::admin_api::auth::handle_admin_login(...)` etc. No routing
change.

## Acceptance criteria

- [ ] `admin_api/auth.rs` and `admin_api/bootstrap.rs` exist with the
      listed contents.
- [ ] `server.rs` no longer defines any of the listed items.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.

## Out of scope

- Refactoring the bootstrap flow itself (the locking and sentinel
  logic stays).
- Combining `LoginResponse` and `WhoamiResponse` even though they
  partially overlap.
