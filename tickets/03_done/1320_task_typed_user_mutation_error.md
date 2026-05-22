# Task: Replace `msg.contains(...)` user-CRUD error sniffing with a typed `UserMutationError`

Parent: `1300_story_backend_backendd_refinement.md`
Resolves: F-005 (user-CRUD half) (review report `backend_backendd_2026-05-22.md`)

## Background

The four user handlers in `admin_api/users.rs` and the password-change
handler in `admin_api/auth.rs` pick the HTTP status code + structured
`code` for their response by string-matching the `anyhow::Error`'s
rendered message:

- `users.rs:187-205` (create): `msg.contains("already exists")`.
- `users.rs:252-277` (update): `msg.contains("not_found")` /
  `msg.contains("not found")` / `msg.contains("already exists")`.
- `users.rs:336-355` (reset_password): `msg.contains("not_found")`.
- `users.rs:388-414` (delete): `msg.contains("not_found")` /
  `msg.contains("last_user_protected")`.
- `auth.rs:307-316` (password change): same `not_found` sniff
  pattern via `mutate_collection`'s `bail!("not_found")`.

The matched strings are produced in `users.rs:122-127, 154-163,
178-179` by `anyhow::bail!`. If anyone reflows the bail message,
the HTTP status mapping silently flips back to a generic 400 or 500.

## Scope

### Backend crate

- Introduce `UserMutationError` in `crates/backend-backendd/src/users.rs`:

  ```rust
  #[derive(Debug, thiserror::Error)]
  pub enum UserMutationError {
      #[error("user '{0}' not found")]
      NotFound(String),
      #[error("user '{0}' already exists")]
      DuplicateUsername(String),
      #[error("cannot delete the last remaining user")]
      LastUserProtected,
      #[error("username must not be empty")]
      InvalidUsername,
      #[error("password_hash must be a PHC-encoded argon2 hash")]
      BadHash,
  }
  ```

  Add `thiserror = "1"` to the crate's `[dependencies]` (workspace
  dep if one exists; otherwise pin the latest 1.x).
- Change `UserCollection::add`, `update`, `remove` to return
  `Result<_, UserMutationError>` (or `Result<&UserRecord, ...>`
  where the prior signature did). Wire through to the same
  invariants the current `bail!` lines enforce.
- Update `mutate_collection` in `admin_api/users.rs` to be generic
  over a result that can be either `UserMutationError` or
  `anyhow::Error`, or split into two helpers (`mutate_collection`
  for I/O failures, `mutate_users` for the typed mutation
  errors). Whatever shape is cleanest — the existing closure
  signature is the friction point.
- Each of the five handlers matches on the variant instead of the
  string-sniff. The HTTP status codes and the structured `code`
  values stay byte-for-byte identical:
  - `NotFound` → 404 `not_found`.
  - `DuplicateUsername` → 409 `username_already_exists`.
  - `LastUserProtected` → 409 `last_user_protected`.
  - `InvalidUsername` → 400 `invalid_user`.
  - `BadHash` → 400 `invalid_user` (or 500 `internal` if hashing
    failed — verify against the current branch).

### Runtime-side `schema_version` sniff stays

`runtime_api/invocations.rs:320-340` matches
`msg.contains("schema_version")` on a `ResourceBlob::from_canonical_bytes`
error. That needs a typed error in `shared-store` — explicitly out
of scope. The user owns that separate non-refactor ticket.

### Tests

- The existing tests in
  `crates/backend-backendd/src/users/tests.rs` and
  `crates/backend-backendd/src/admin_api/users/tests.rs` should pass
  unchanged once they read the new error type. Where a test asserts
  on the rendered message string (e.g.
  `assert!(err.to_string().contains("already exists"))`), update to
  match the variant directly:

  ```rust
  assert!(matches!(err, UserMutationError::DuplicateUsername(_)));
  ```

## Acceptance criteria

- [ ] No `msg.contains("not_found" | "already exists" |
      "last_user_protected" | "not found")` call survives in
      `crates/backend-backendd/src/admin_api/users.rs` or
      `crates/backend-backendd/src/admin_api/auth.rs`.
- [ ] `UserMutationError` exists and is returned by
      `UserCollection::{add, update, remove}`.
- [ ] HTTP status codes and structured `code` strings for every
      user-CRUD failure path match the current behaviour byte-for-byte.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
