# Task: Update docs to match the new architecture

Parent: `260_story_cutover_and_documentation.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Update `CLAUDE.md`:
  - Remove references to a Rust `underwrap-admin-ui` crate (already stale).
  - Replace the Next.js BFF description with the new single-binary architecture.
  - Update "Key files to understand first" list (drop Next.js entries, add `crates/backend-backendd/src/auth.rs`, `web.rs`, `operators.rs`).
- Update `docs/security.md`:
  - Admin listener TLS termination.
  - Multi-user model: operator records, roles, per-store grants, Argon2id + AEAD session cookies, CSRF.
  - Declarative-vs-dynamic stores threat model (declarative store config trusted as part of the system image; dynamic store config trusted as backend-written).
- Update `docs/backend.md`:
  - Stores config directory model (declarative + dynamic).
  - Auth flow (login, bootstrap, logout, CSRF).
  - Stores CRUD API.
  - Users CRUD API.
- Update `README.md` with the simplified deployment story.
- Add an "Architecture overview" markdown diagram (mermaid or ASCII) showing browser ↔ backendd (TLS) ↔ stores.

## Acceptance criteria

- [ ] All four docs reflect the new architecture, with no stale Next.js or BFF references.
- [ ] Security invariants enumerated and mapped to the mechanisms that enforce them.
- [ ] A diagram shows the new architecture.
- [ ] `docs/security.md` "Never do" / "Always do" lists updated to include the new admin listener and multi-user rules.
