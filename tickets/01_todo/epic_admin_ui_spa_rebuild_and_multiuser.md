# Epic: Rebuild the admin UI as a Rust-served SPA with multi-user and stores management

## Background

The current Next.js admin UI in `apps/admin-ui/` is a Node.js BFF sitting between the browser and `backend-backendd`'s admin API. This was a reasonable starting point but:

- Node.js + the npm dep tree expands the TCB of a security-focused project.
- The shared admin bearer token between the BFF and backendd lets the BFF impersonate any operator; per-operator attribution is awkward.
- Two processes, two configs, two systemd units, and a loopback invariant that has to keep holding — all for what is really a thin translation layer.
- The current UI is single-user (one operator config), single-store-source (stores declared once in backendd YAML), and the access matrix lives inside a per-store wrapper page.

This project is **greenfield**. For this work, we should optimize for the best first-release operator experience, a smaller TCB, and a single self-contained binary.

## Product decisions already made

- Replace the Next.js BFF with a **browser SPA** that talks directly to `backend-backendd`. No Node.js in the runtime closure.
- New SPA stack: **Vite + React 19 + TypeScript + Tailwind v4 + shadcn/ui + TanStack Router + TanStack Query + react-hook-form + zod**. Lucide icons. Vitest + Playwright.
- Rust→TS type generation via **ts-rs** (a `cargo test` step emits `web/src/api/types.ts`).
- Backend serves the SPA bundle itself via an embedded asset store (`rust-embed`) on a TLS-terminated admin listener (rustls, same machinery as the service listener).
- Operator records (multi-user) move into the backend store; per-operator role (`admin` / `operator`) and per-store access grants are first-class.
- Stores are managed either **declaratively** (NixOS module materializes config as symlinks from `/nix/store` into `/etc/underwrap-backend/stores.d/`) **or dynamically** (created from the UI under `/var/lib/underwrap-backend/stores.d/`). Backend refuses to mutate declarative store definitions.
- The **access matrix becomes a top-level navigation tab**, not a per-store sub-page.
- **Policy model simplified**: per-secret mutation-windows are removed entirely. Agent grants live at the `(wrapper, host, user)` tuple level, each with one of the 4 existing wrapper modes. Read access through an active grant is unconditional. Write access is gated by a **single global `writes_enabled` toggle** — no per-secret or per-grant write ACL. See story 245.
- UI must be **mobile-friendly** (≥375 px width, sidenav-as-drawer, table-to-card fallbacks).

## Goals

1. Single self-contained `underwrap-backendd` binary serves both the admin API and the SPA.
2. No bearer-token middleman; operator sessions are the authentication identity end-to-end.
3. Multi-user support: an admin operator can create operators and grant them per-store access.
4. Operators can create and manage stores from the UI; declarative NixOS-managed stores remain supported.
5. Access matrix moves into its own tab.
6. Mobile-friendly across all pages.
7. Strong test coverage at every layer: Rust unit + integration, Vitest, Playwright, NixOS VM.

## Security invariants

- Browser ↔ backendd is **TLS-encrypted end to end** on the admin listener.
- Session cookies remain `HttpOnly`; state-changing requests require CSRF + same-origin enforcement.
- Operator passwords stored as Argon2id PHC hashes in the store, never logged.
- Secret values never appear in URLs, localStorage, sessionStorage, or logs.
- Reveal/download/mutation actions emit explicit audit events including the operator principal.
- No client-side fallback may bypass server-side authorization.
- Per-store ACL filtering happens server-side on every store-scoped endpoint; the UI hiding controls is a UX nicety, not the security boundary.
- Declarative store config files are read-only from the backend's perspective; the backend refuses to write them and rejects API mutations targeting them.

## Scope

- Backend serves the SPA, with admin TLS.
- Auth + sessions + CSRF in Rust.
- Multi-user (admin/operator roles, per-store ACLs).
- Stores management (declarative + dynamic).
- Agent grant model rewrite: remove per-secret mutation windows, replace with `(wrapper, host, user) → mode` grants and a global `writes_enabled` toggle.
- New SPA scaffold and all pages.
- Access matrix as a top-level tab, built against the new grant model.
- Mobile responsive pass.
- ts-rs type generation.
- Cutover: removal of the Next.js app and Node-based packaging.
- Updated NixOS VM test.
- shame01 deployment migration.

## Out of scope

- OIDC / SSO / external identity providers.
- Fine-grained per-secret or per-wrapper ACLs (store-level ACL is the unit).
- Reworking the frontend/backend wrapper chain or the mTLS service-listener model.

## Stories

Execution order:

1. `200_story_backendd_serves_spa_and_admin_tls.md`
2. `210_story_rust_native_auth_sessions_multiuser.md`
3. `220_story_vite_react_shadcn_spa_scaffold.md`
4. `230_story_stores_management_declarative_and_dynamic.md`
5. `240_story_multi_user_with_per_store_access.md`
6. `245_story_agent_grant_model_rewrite.md`
7. `250_story_access_matrix_tab_and_mobile_polish.md` (depends on 245)
8. `260_story_cutover_and_documentation.md`

Plus a final standalone task once everything above is green:

9. `270_task_shame01_nixos_config_migration.md`

Within each story, execute its `NNN_task_...` children in ascending order unless a later ticket is explicitly pulled forward.

## Acceptance criteria

- [ ] One self-contained Rust binary serves the admin API and the SPA over TLS.
- [ ] No Node.js in the runtime closure of either flake output or systemd unit.
- [ ] Argon2id-verified operator login with signed session cookies; logout and CSRF protection in Rust.
- [ ] Admin operators can create/delete operators and toggle role + per-store grants from the UI.
- [ ] Operators see only stores they are granted; admins see all.
- [ ] Stores can be created/edited/deleted from the UI (dynamic) or declared in NixOS (declarative); backend enforces declarative-store immutability.
- [ ] Per-secret mutation windows removed; agent grants are `(wrapper, host, user) → mode` only; a single global `writes_enabled` toggle gates all writes.
- [ ] Access matrix is a top-level tab whose cells are grant modes (not per-secret toggles).
- [ ] Every page renders usably at 375×812.
- [ ] ts-rs generates `web/src/api/types.ts`; CI fails on drift.
- [ ] NixOS VM test boots a real host, bootstraps an admin via the SPA, exercises multi-user, declarative-and-dynamic stores, reveal, mutation, and audit events.
- [ ] Playwright covers login, multi-user, store CRUD (dynamic + declarative-disabled), matrix tab, mobile drawer nav.
- [ ] `~/nix/my_nix_os/hosts/shame01/configuration.nix` migrated to the new architecture with a documented runbook.
- [ ] The old `apps/admin-ui/` Next.js app and `admin-ui-nextjs` Nix module are removed.
- [ ] Docs (`CLAUDE.md`, `docs/security.md`, `docs/backend.md`, `README.md`) reflect the new architecture.
