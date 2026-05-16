# Task: Users page and grant editor

Parent: `240_story_multi_user_with_per_store_access.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- `/users` route, admin-only; nav item hidden for non-admin sessions.
- Table on desktop / cards on mobile: username, role badge, granted stores (chip list), created_at, password_changed_at.
- Create-user dialog: username, password (with strength hint), confirm-password, role select, grants multi-select.
- Edit-user dialog: change role, grants, username (no password change here).
- Reset-password dialog: admin sets a new password for the user; one-time display option (optional convenience) or just confirmation.
- Delete confirmation with last-admin guard messaging surfaced inline.
- `/profile` self-edit page for any operator to change their own password (requires current password).

## Acceptance criteria

- [ ] All flows work on desktop and mobile.
- [ ] Last-admin server error rendered with a friendly inline message naming the constraint.
- [ ] Vitest tests cover form validation, last-admin disabled state, password strength hint.
- [ ] Playwright covers: admin creates operator → operator logs in → operator-side nav hides Users/Stores → operator changes own password via /profile → re-login with new password works.
- [ ] No password ever appears in the URL, in localStorage, or in any list response.
