# Task: Implement login, sessions, logout, and route protection

Parent: `030_story_admin_ui_password_auth_sessions_and_csrf.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Implement username/password login with file-backed Argon2 hashes.
- Implement signed session cookies with expiry.
- Protect authenticated routes.
- Implement logout.

## Acceptance criteria

- [ ] Valid credentials create a session.
- [ ] Invalid credentials are rejected.
- [ ] Authenticated routes require a valid session.
- [ ] Logout clears the session.
- [ ] Automated tests cover success, failure, and expiry behavior.
