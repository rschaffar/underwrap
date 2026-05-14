# Story: Implement password auth, session handling, and CSRF for the Next.js admin UI

Epic: `tickets/01_todo/epic_nextjs_admin_ui_replatform.md`

## Background

We are intentionally keeping the current simple operator auth model:
username + password backed by Argon2 PHC hashes in files. The new UI
must preserve the current security posture while moving to a JS-based
server runtime.

## Scope

- Load admin UI runtime config, including:
  - listen/bind settings as applicable
  - session signing key
  - operator password-hash files
  - backend target token files
- Implement server-side username/password verification.
- Implement signed operator sessions with expiry.
- Implement logout.
- Implement CSRF protection and/or strong same-origin request checks for
  every state-changing action.
- Protect authenticated routes.
- Decide and document the Argon2 verification approach in a Nix-friendly
  way without casually introducing problematic native runtime baggage.

## Acceptance criteria

- [ ] Operators can log in with file-backed Argon2 hashes.
- [ ] Wrong-password login is rejected cleanly.
- [ ] Authenticated pages require a valid session.
- [ ] Logout clears the session.
- [ ] State-changing requests are rejected when CSRF validation fails.
- [ ] Session cookies are `HttpOnly` and not readable from frontend JS.
- [ ] Automated tests cover login success/failure, expired sessions, and
      CSRF rejection paths.

## Child tasks

- `030_task_admin_ui_server_config_loader_and_backend_client.md`
- `040_task_admin_ui_login_sessions_and_route_protection.md`
- `050_task_admin_ui_csrf_same_origin_and_state_change_guards.md`
