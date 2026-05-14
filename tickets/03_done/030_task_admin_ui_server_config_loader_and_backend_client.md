# Task: Implement server-side config loading and backend client for the Next.js app

Parent: `030_story_admin_ui_password_auth_sessions_and_csrf.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Load admin UI runtime config for operators, session keys, backend URLs,
  and backend admin tokens.
- Keep backend tokens server-side only.
- Implement the server-side client layer the Next.js app uses to call
  `backend-backendd`.
- Define safe error handling that does not leak secrets or tokens.

## Acceptance criteria

- [ ] Config loading works for operator hashes, session keys, and backend tokens.
- [ ] Backend tokens never cross into browser-visible responses.
- [ ] The app has a reusable server-side backend client layer.
- [ ] Error paths avoid leaking sensitive material.
