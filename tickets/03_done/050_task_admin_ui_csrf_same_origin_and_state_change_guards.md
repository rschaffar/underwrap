# Task: Add CSRF and same-origin protections for all state-changing actions

Parent: `030_story_admin_ui_password_auth_sessions_and_csrf.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Protect all mutating requests with CSRF and/or strong same-origin checks.
- Ensure forms/actions used by the new UI fail closed.
- Cover wrapper mode, matrix, mutation, write, and delete flows.

## Acceptance criteria

- [ ] State-changing requests are rejected when protection checks fail.
- [ ] Protection covers all operator write paths.
- [ ] Tests exercise rejection paths for the main mutating actions.
