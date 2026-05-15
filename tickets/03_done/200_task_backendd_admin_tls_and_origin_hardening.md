# Task: Add TLS termination and origin hardening to the backendd admin listener

Parent: `200_story_backendd_serves_spa_and_admin_tls.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Extend `AdminConfig` to accept an optional TLS configuration with the same shape as `ServiceTlsConfig` (cert file, key file). Leave optional client_ca off by default — mTLS for the admin listener is out of scope for v1 since the auth identity is the session cookie.
- Reuse `build_tls_acceptor` (or factor a shared helper) to terminate TLS on the admin listener when configured.
- Add an optional `allowed_origins` list. State-changing requests (`POST/PUT/DELETE/PATCH`) must carry an `Origin` (falling back to `Referer`) that matches. Used by the CSRF work in ticket 212 — wire the plumbing here.
- Default config refuses to start in a configuration that would allow non-loopback plain-HTTP browser traffic (no TLS, listen address not loopback). Fail-closed against accidental plain-HTTP exposure.

## Key files likely involved

- `crates/backend-backendd/src/server.rs` (`AdminConfig`, `admin_accept_loop`, `handle_admin_connection`, `build_tls_acceptor`)
- New module `crates/backend-backendd/src/admin_tls.rs` if factoring helps
- New tests under `crates/integration-tests/`

## Handoff notes

- The service listener already uses rustls + `ring` crypto provider + `subtle` constant-time comparison; reuse those primitives where possible to keep the crypto surface small.
- `rustls::crypto::ring::default_provider().install_default()` must remain a no-op on repeat calls (it currently runs in `build_tls_acceptor`).
- Do not change the service listener's mTLS contract; this ticket only touches the admin listener.

## Acceptance criteria

- [ ] `AdminConfig` accepts an optional `tls` block; when present, the admin listener wraps incoming connections with rustls.
- [ ] `AdminConfig` accepts an optional `allowed_origins` list and refuses to apply it without TLS (or a loopback listen address).
- [ ] Backend refuses to start in a configuration that would expose non-loopback plain HTTP.
- [ ] Rust unit tests cover: startup-time config validation matrix, `Origin`/`Referer` allowlist (allowed, mismatch, missing).
- [ ] Rust integration test exercises a full TLS handshake against the admin listener and verifies a bearer-token request still succeeds (bearer-token auth is replaced in ticket 211; until then, the existing token path remains).
- [ ] No regressions on `cargo test -p backend-backendd` or `nix build .#checks.x86_64-linux.vm-test`.
