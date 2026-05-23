# Task: Move password length rule into the backend; drop SPA-side `.min(N)`

## Background

Password length is enforced inconsistently:

- `web/src/pages/bootstrap.tsx:21` — `MIN_PASSWORD_LEN = 12`
- `web/src/pages/profile.tsx:32` — `.min(8, …)`
- `web/src/pages/users/dialogs/create.tsx:42` — `.min(8, …)`
- `web/src/pages/users/dialogs/reset-password.tsx:34` — `.min(8, …)`
- Backend: only `if body.password.is_empty()` rejects
  (`crates/backend-backendd/src/admin_api/users.rs:188-191, 275-278`).
- OpenAPI advertises "password too short" (`auth/bootstrap.rs:86`,
  `auth/mod.rs:197`) but no length check exists.

Password handling is a backend security boundary. The
authoritative rule lives there.

Source review:
`tickets/_review/web_admin_spa_2026-05-23.md` F-001.

## Scope

### Backend — add the rule

In `crates/backend-backendd/src/auth.rs`, add:

```rust
/// Minimum password length enforced by every admin handler that
/// consumes a raw password (bootstrap, login does not touch this
/// rule, password change, user create, user reset-password).
/// 12 was the bootstrap form's pre-cutover value; we standardise on
/// it as the single authoritative rule.
pub const MIN_PASSWORD_LEN: usize = 12;

/// Validate an operator-supplied password before hashing. Returns
/// `Ok(())` on accept, or an error code suitable for the
/// `password_too_short` / `password_empty` envelope.
pub fn validate_password(password: &str) -> Result<(), &'static str> {
    if password.is_empty() {
        return Err("password_empty");
    }
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err("password_too_short");
    }
    Ok(())
}
```

(Use `chars().count()` so multi-byte UTF-8 doesn't make a 12-char
password fail on its 8-byte count.)

### Backend — call `validate_password` from every entry point

Replace the existing `is_empty()` checks with `validate_password`:

- `crates/backend-backendd/src/admin_api/auth/bootstrap.rs` — in
  `handle_admin_bootstrap`, run before `hash_password`.
- `crates/backend-backendd/src/admin_api/auth/mod.rs:202+` —
  `handle_admin_password_change`, on the new password field.
- `crates/backend-backendd/src/admin_api/users.rs:188-191` —
  `handle_admin_user_create`.
- `crates/backend-backendd/src/admin_api/users.rs:275-278` —
  `handle_admin_user_reset_password`.

Each emits `AdminError::bad_request("password_too_short", …)` or
`AdminError::bad_request("password_empty", …)` as appropriate.

Do **not** add the rule to `handle_admin_login` — login matches an
existing hash; rejecting on length there would just leak the
password length to a probe.

### Backend — update OpenAPI descriptions

`auth/bootstrap.rs:86` and `auth/mod.rs:197` already say "password
too short". Leave them; the rule now matches the description.
`users.rs` response docs say "Validation failed" — extend to
mention `password_too_short` so the generated SPA error mapping
has the stable code.

### SPA — extend `describeApiError`

In `web/src/lib/error-message.ts`, add cases for the new codes:

```ts
case "password_too_short":
  return "Password must be at least 12 characters.";
case "password_empty":
  return "Password is required.";
```

Add the codes to `web/src/lib/error-message.test.ts` alongside the
other coverage cases.

### SPA — drop the form-side `.min(N)` constraints

- `web/src/pages/bootstrap.tsx:21-28`: drop the
  `MIN_PASSWORD_LEN` constant and the `.min(MIN_PASSWORD_LEN, …)`
  refinement. Replace the form description ("At least 12
  characters.") inline since the constant is gone.
- `web/src/pages/profile.tsx:32`: change to
  `z.string().min(1, "Current password is required.")` on the
  current-password field (still need non-empty client-side);
  remove the `.min(8, …)` from the new-password field.
- `web/src/pages/users/dialogs/create.tsx:42`: `z.string()`
  (rely on backend; or keep `.min(1)` for the "is required"
  message — judgement, pick `.min(1)`).
- `web/src/pages/users/dialogs/reset-password.tsx:34`: same as above.

The form descriptions ("At least 8 characters", "Longer is
better") should be updated to "At least 12 characters." across
all four places — same text, sourced from the new authoritative
rule. Inline it; do not import a constant from the backend.

## Out of scope

- Adding password complexity rules (digits, symbols). The
  minimum length is the only rule.
- Login-path rate limiting changes.
- Touching the Argon2id parameters.

## Acceptance criteria

- [ ] `grep -n "MIN_PASSWORD_LEN" web/src/` returns zero hits.
- [ ] `grep -rn "\.min(8" web/src/pages/` returns zero hits.
- [ ] `grep -rn "\.min(MIN_PASSWORD_LEN\|\.min(12" web/src/` returns
      zero hits.
- [ ] `crates/backend-backendd/src/auth.rs` exports
      `MIN_PASSWORD_LEN` and `validate_password`.
- [ ] Backend unit tests exist for the boundary cases (11 chars
      rejected, 12 accepted, empty rejected) on every handler that
      consumes a password. Live alongside the other handler tests
      in `admin_api/users/tests.rs`, `admin_api/auth/tests.rs`,
      and `admin_api/auth/bootstrap/tests.rs`. Use the sibling
      `tests.rs` pattern.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes (`auth-flows.spec.ts`
      and `users.spec.ts` cover password flows).
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.

## Notes for the implementer

- 12 was already in use for bootstrap. Picking it makes the
  cutover one-directional: any password the SPA previously
  required has 12+ chars; only the operator-typed passwords with
  8–11 chars from `users/create.tsx` / `profile.tsx` /
  `reset-password.tsx` break. That window is short enough to
  accept per the greenfield rules.
- The OpenAPI drift test regenerates `openapi.json`; commit the
  diff alongside.
- The same SPA pages then need `npm run generate:api` to refresh
  `schema.ts`. Commit that diff too.
