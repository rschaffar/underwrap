# Task: Doc, dep, and remnant-cleanup sweep for `backend-backendd`

Parent: `1300_story_backend_backendd_refinement.md`
Resolves: F-004, F-007, F-008, F-014, F-015, F-016, F-017, F-020
  (review report `backend_backendd_2026-05-22.md`)

## Background

Catch-all ticket for the small wins surfaced in the 2026-05-22
review that don't merit individual tickets. Each sub-bullet is
independent.

## Scope

### F-004 — drop unused `_stores: &[&str]` test param

- `crates/backend-backendd/src/test_support.rs:444-449` —
  `admin_principal(_stores: &[&str])` is always called with `&[]`
  (see callers in `test_support.rs` and every `tests.rs` file under
  `admin_api/`).
- Drop the parameter. Update every `admin_principal(&[])` call site
  to `admin_principal()`.

### F-007 — stale `Cargo.toml` description

- `crates/backend-backendd/Cargo.toml:3` says "holds secrets, serves
  them over Unix socket (for SSH tunnel)". Replace with:

  ```
  description = "underwrap backend daemon — HTTPS service that holds \
  secrets, serves the runtime mTLS API, and hosts the admin SPA + JSON API"
  ```

  (or any equivalent phrasing that's accurate today).

### F-008 — duplicate `tempfile` dev-dep

- `crates/backend-backendd/Cargo.toml:48, 51` — `tempfile` appears in
  both `[dependencies]` and `[dev-dependencies]`. Delete the
  `[dev-dependencies]` entry; the regular dep already covers tests.

### F-014 — `whoami` swallows user-collection load errors

- `crates/backend-backendd/src/admin_api/auth.rs:225` —
  `crate::users::load(&state.users_file).unwrap_or_default()`
  silently masks a disk error and returns `authenticated: false`.
- Keep the current behaviour (the SPA's redirect dance depends on
  whoami not 500ing on a stale session) **but** add a `warn!` so
  operators can see the underlying error in the logs:

  ```rust
  let collection = crate::users::load(&state.users_file).unwrap_or_else(|e| {
      warn!(err = %e, "whoami: user collection unreadable; treating as empty");
      crate::users::UserCollection::default()
  });
  ```

### F-015 — missing module doc on `runtime_clients.rs`

- `crates/backend-backendd/src/runtime_clients.rs:1` — file opens
  directly on `use anyhow::{...}` while every other module has a
  `//!` summary.
- Add a `//!` block summarising `ClientInstanceRecord`,
  `CurrentCredentialRecord`, `ClientInstancesDocument`, and how they
  feed the runtime mTLS handshake.

### F-016 — `epic_backend_modularization` pointer in source

- `crates/backend-backendd/src/runtime_state.rs:12-13` —
  `//! Splitting this struct into smaller per-concern structs is
  deferred — see `epic_backend_modularization`.` The epic is closed
  and ticket / epic pointers in source are forbidden by the posture.
- Either drop the sentence entirely, or rephrase the design note
  without naming the epic.

### F-017 — legacy inline `stores:` rejection in `main.rs`

- `crates/backend-backendd/src/main.rs:53-69` — `parse_backend_config`
  re-parses YAML once to detect the old inline `stores:` shape and
  bail with a migration hint. No such config was ever deployed.
- Delete the pre-check; fold `parse_backend_config` back to a
  single `serde_yaml::from_str` call. Delete the matching test at
  `:193-208`
  (`parse_backend_config_rejects_legacy_inline_stores_with_migration_hint`).

### F-020 — "Next.js BFF" remnants in `auth.rs`

- `crates/backend-backendd/src/auth.rs:7` and `:30-32` reference
  the Next.js BFF that's gone. Strip the parentheticals; describe
  the session-key file as 32 random bytes used as the AEAD key.

## Acceptance criteria

- [ ] `rg -n 'admin_principal\(&\[\]\)' crates/backend-backendd/src/`
      returns nothing.
- [ ] `Cargo.toml` description no longer references "Unix socket".
- [ ] `tempfile` appears in `[dependencies]` only.
- [ ] `whoami` emits a `warn!` on user-collection load failure.
- [ ] `crates/backend-backendd/src/runtime_clients.rs` opens with
      a `//!` doc block.
- [ ] `rg -n 'epic_backend_modularization' crates/` returns nothing.
- [ ] `rg -n 'inline.*stores:' crates/backend-backendd/src/main.rs`
      returns nothing; the legacy-config test is gone.
- [ ] `rg -ni 'Next.?js' crates/backend-backendd/src/` returns
      nothing.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.
