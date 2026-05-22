# Task: Extract a single session-issuing helper for login + bootstrap

Parent: `1300_story_backend_backendd_refinement.md`
Resolves: F-012 (review report `backend_backendd_2026-05-22.md`)

## Background

`admin_api/auth.rs::handle_admin_login` (lines 110-156) and
`admin_api/bootstrap.rs::handle_admin_bootstrap` (lines 186-225)
each carry ~30 lines of identical session-issuing logic: build a
`SessionPayload`, encrypt it under `state.session_key`, render the
session cookie + the CSRF cookie, build a `Response::builder()` with
two `SET_COOKIE` headers and the JSON body. The two functions differ
only in:

- The success JSON body type (`LoginResponse` vs `BootstrapResponse`).
- The success log message ("login succeeded" vs "bootstrap: first
  user created").

`test_support::run_bootstrap` (lines 357-442) carries a third
hand-rolled copy.

## Scope

- Extract a helper, e.g.:

  ```rust
  pub(crate) fn issue_session_response<B: Serialize>(
      state: &AdminRuntimeState,
      payload: &SessionPayload,
      body: &B,
  ) -> Response<Full<Bytes>>
  ```

  Location: a new private function in `admin_api/auth.rs`, or
  promote to a small `admin_api/sessions.rs` if that reads cleaner.
  The helper handles `encrypt_session`, `render_session_cookie`,
  `render_csrf_cookie`, and the `Response::builder()` with two
  `SET_COOKIE` headers.
- Replace the bodies of `handle_admin_login` and
  `handle_admin_bootstrap` with one call each. The login / bootstrap
  pre-checks (validation, rate-limit, sentinel) stay where they are.
- Replace the `test_support::run_bootstrap` copy with the same call.
- If `encrypt_session` failure can no longer surface from the
  caller (because the helper handles it internally), keep a 500 path
  inside the helper or return `Result` and unwrap at the two real
  call sites — your call.

## Out of scope

- Changing the cookie attributes, lifetime, or AEAD construction.
- Changing the login/bootstrap pre-check logic.

## Acceptance criteria

- [ ] No `encrypt_session` + `render_session_cookie` +
      `render_csrf_cookie` triple survives outside the new helper.
- [ ] `cargo test --workspace` passes (login + bootstrap unit tests
      should keep passing unchanged).
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
