# Task: Encrypted-store key one-time reveal flow

Parent: `230_story_stores_management_declarative_and_dynamic.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- When creating an encrypted store via the admin API, backend:
  - Generates a fresh key (libsodium-compatible secret-box key or equivalent for the chosen at-rest cipher).
  - Writes it to a backend-controlled key file `/var/lib/underwrap-backend/keys/<store>.key`, mode 0600, atomically.
  - Returns the key value **once** in the create-store response.
- Subsequent reads of the store definition never include the key value.
- `POST /v1/admin/stores/{name}/rotate-key` rotates the key (audited; one-time reveal in the response). Old key remains usable for re-encryption work — out of scope for v1 of rotation; document the limitation.
- Frontend create dialog shows the key in a one-time reveal modal with:
  - Copy-to-clipboard button
  - Acknowledgement checkbox ("I have saved this key securely")
  - Confirm-and-close action that drops the value from React state and triggers a refresh
- Key value never written to `localStorage`, `sessionStorage`, or URLs. Verified by Playwright inspecting storage at the end of the flow.

## Acceptance criteria

- [ ] Backend exposes key value exactly once per generation or rotation.
- [ ] Key file written with mode 0600 atomically.
- [ ] UI flow forces the acknowledgement before closing the modal.
- [ ] Tests cover: key generated on create, no key in subsequent GETs, rotate-key returns a new key once, file mode 0600 verified, audit emission for create and rotate, Playwright verifies storage cleanliness after reveal flow.
