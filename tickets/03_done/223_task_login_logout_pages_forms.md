# Task: Login, logout, and bootstrap pages

Parent: `220_story_vite_react_shadcn_spa_scaffold.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- `/login` page: username/password form using shadcn `Form` + react-hook-form + zod. On success, redirect to `?next=` or `/`.
- `/logout` route: triggers `POST /v1/admin/auth/logout`, then redirects to `/login`.
- `/bootstrap` page: shown when `bootstrap_required`; username + password + confirm-password; same form components; on success → logged in as admin → `/`.
- Generic error toast for backend `{code, message}` errors; specific inline messages for `invalid_credentials`, `rate_limited`, `bootstrap_already_completed`.
- Password input fields use `autocomplete="current-password"` / `new-password` correctly.

## Acceptance criteria

- [ ] All three pages render correctly on mobile (375×812) and desktop.
- [ ] Form validation fires before submit (empty fields, password length, password mismatch on bootstrap).
- [ ] Vitest tests cover form validation, success path mock, and error display.
- [ ] All forms are keyboard-accessible: tab order is sensible, Enter submits, focus trap on dialogs.
- [ ] No password value ever logged or sent anywhere other than the auth endpoint.
