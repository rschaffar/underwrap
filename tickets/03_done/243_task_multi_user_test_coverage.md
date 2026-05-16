# Task: Multi-user regression and authz negative test coverage

Parent: `240_story_multi_user_with_per_store_access.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Rust integration tests:
  - operator-A cannot read operator-B
  - operator cannot reach `/v1/admin/users` at all (admin-only guard)
  - operator with grant to store-A but not store-B: every store-scoped endpoint 404s for store-B
  - admin sees all
  - password hash never appears in any list or get response, nor in tracing logs (verified by capturing the `tracing` subscriber output during user CRUD)
- Playwright e2e:
  - bootstrap → admin → create operator with grant to store-A → admin logout
  - operator login → sees store-A only, cannot see store-B in any list, nav hides Users + Stores
  - operator changes own password → login with new password works
  - operator cannot navigate directly to `/users` or `/stores` (router redirects or shows forbidden)
- NixOS VM test extended to exercise the same scenario end-to-end on a real host.

## Acceptance criteria

- [ ] All listed tests pass in `cargo test --workspace`, `nix flake check`, and `nix build .#checks.x86_64-linux.vm-test`.
- [ ] At least one timing-stability assertion that login response time is consistent regardless of user existence (regression for ticket 211).
- [ ] Negative-test sweep over every store-scoped endpoint asserts 404 for ungranted store.
