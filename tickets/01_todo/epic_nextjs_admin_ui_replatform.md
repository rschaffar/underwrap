# Epic: Replace the Rust admin UI with a modern Next.js operator console

## Background

The current admin UI lives in `crates/backend-admin-ui` and is a
server-rendered Axum application with hand-built HTML forms. It is
secure-minded and functional, but the operator experience is clunky,
composition-heavy, and not a strong first-release UI.

This project is still **greenfield**. For this work, we should optimize
for the best first-release operator experience, a cleaner architecture,
and stronger security boundaries where the redesign makes them clearer.
**Backward compatibility with the current Rust UI is not a constraint.**

## Product decisions already made

- Replace the current Rust UI with a **Next.js + TypeScript** web app.
- Keep **username/password** operator authentication for now.
- The new UI **must support revealing secret values**.
- Support **real file upload/download** for file-backed secrets in v1.
- Treat **multi-admin target switching** as a first-class workflow.
- The visual target is a **dark, high-contrast ops console**.
- Ship a **first-class NixOS integration path** immediately.
- This is a **full UX rethink**, not a parity-only reskin.
- We will follow **Option B**: improve or add backend admin API surfaces
  as needed so the UI is built on clean view-model endpoints instead of
  awkward client-side orchestration.

## Goals

1. Deliver a modern operator UI for resources, wrapper modes, mutation
   windows, and access policy.
2. Keep backend bearer tokens strictly server-side; the browser must
   never talk to `backend-backendd` directly.
3. Support safe, deliberate secret reveal flows for env, files, and
   libsecret entries.
4. Replace the current form-heavy pages with searchable, filterable,
   task-oriented workflows.
5. Package and deploy the new UI cleanly in the existing Nix-based
   project.
6. Remove the old Rust UI after the new app is ready.

## Security invariants

- Browser never receives backend admin bearer tokens.
- Secret values are never placed in URLs.
- Secret values are never written to localStorage/sessionStorage.
- Secret reveal is operator-initiated and masked by default.
- UI reveal/download actions are explicitly audited by the backend.
- Session cookies remain `HttpOnly`; state-changing requests require
  CSRF protection and/or same-origin enforcement.
- Audit remains backend-owned; the UI does not become the system of
  record.
- No client-side fallback path may bypass server-side authorization.
- The UI must handle non-UTF-8 secret material safely, e.g. by offering
  base64 reveal/download instead of assuming text.

## Proposed architecture direction

- New app implementation in a dedicated JS/TS app directory (working
  assumption: `apps/admin-ui/` or similar), replacing the current Rust
  UI implementation once cutover is complete.
- Next.js used primarily as a **server-side BFF**:
  - Server Components / server-side loaders for reads
  - route handlers or server actions for writes
  - selective client components only for interaction-heavy widgets
- Backend gains frontend-oriented read-model endpoints and reveal
  endpoints where that materially simplifies the UI.
- Public product/deployment name can remain `underwrap-admin-ui` even if
  the implementation moves out of the Rust workspace.

## Scope

- Information architecture and UX redesign
- New frontend-oriented backend admin API surface
- Next.js app scaffold, packaging, and deployment shape
- Operator auth/session/CSRF layer
- Resource explorer and secret reveal/edit flows
- Wrapper mode management UX
- Access matrix UX
- Read-only, filterable audit history UX
- Security hardening, end-to-end tests, docs, and cutover

## Out of scope

- OIDC / SSO
- Operator role hierarchies / RBAC redesign
- Mobile-first UI
- Reworking the wrapper chain or the core frontend/backend security
  model outside what the UI/API needs

## Initial child stories

- `tickets/01_todo/000_story_admin_ui_information_architecture_and_backend_view_models.md`
- `tickets/01_todo/010_story_nextjs_admin_ui_scaffold_and_nix_packaging.md`
- `tickets/01_todo/030_story_admin_ui_password_auth_sessions_and_csrf.md`
- `tickets/01_todo/090_story_resource_explorer_secret_reveal_and_mutation_flow.md`
- `tickets/01_todo/110_story_wrapper_modes_and_access_matrix_console.md`
- `tickets/01_todo/115_story_admin_ui_audit_history_console.md`
- `tickets/01_todo/120_story_admin_ui_hardening_e2e_and_cutover.md`

## Recommended execution order

1. `000_story_admin_ui_information_architecture_and_backend_view_models.md`
2. `010_story_nextjs_admin_ui_scaffold_and_nix_packaging.md`
3. `030_story_admin_ui_password_auth_sessions_and_csrf.md`
4. `090_story_resource_explorer_secret_reveal_and_mutation_flow.md`
5. `110_story_wrapper_modes_and_access_matrix_console.md`
6. `115_story_admin_ui_audit_history_console.md`
7. `120_story_admin_ui_hardening_e2e_and_cutover.md`

Within those stories, execute the numbered `NNN_task_...` tickets in ascending
order unless a later ticket is explicitly pulled forward for parallel work.

## Implementation handoff status

Completed task tickets already landed in the repo and were moved to `tickets/03_done/`:

- `000_task_admin_ui_rfc_page_map_and_endpoint_contract.md`
- `010_task_admin_ui_nextjs_app_scaffold_and_design_system.md`
- `020_task_admin_ui_nix_build_runtime_and_dev_shell.md`
- `030_task_admin_ui_server_config_loader_and_backend_client.md`
- `040_task_admin_ui_login_sessions_and_route_protection.md`
- `050_task_admin_ui_csrf_same_origin_and_state_change_guards.md`
- `060_task_admin_ui_backend_overview_and_resource_index_endpoints.md`
- `070_task_admin_ui_backend_resource_detail_reveal_and_binary_download_endpoints.md`
- `080_task_admin_ui_backend_wrapper_matrix_and_mutation_view_endpoints.md`
- `090_task_admin_ui_resource_explorer_and_navigation.md`

Current repo state to know before continuing:

- The new app lives in `apps/admin-ui/`.
- It already has:
  - Next.js App Router scaffold
  - dark shell and navigation
  - server-side runtime config loading from `UNDERWRAP_ADMIN_UI_CONFIG`
  - signed login/session/logout flow
  - CSRF/same-origin protection helpers
  - live dashboard + target overview + resource list + resource detail pages
  - Nix package `underwrap-admin-ui-nextjs`
  - NixOS module `nixosModules.admin-ui` / `nix/modules/admin-ui-nextjs.nix`
- Backend UI read-model endpoints already implemented:
  - `GET /v1/admin/ui/overview`
  - `POST /v1/admin/ui/resources/query`
  - `POST /v1/admin/ui/resource/get`
  - `POST /v1/admin/ui/resource/reveal`
  - `POST /v1/admin/ui/resource/download`
  - `POST /v1/admin/ui/mutation/get`
  - `POST /v1/admin/ui/wrappers/query`
  - `POST /v1/admin/ui/matrix/query`
- Backend tests currently cover those read-model endpoints.

Important handoff/gotcha notes:

- When using `nix build` on this flake, newly created frontend files must be
  **git-tracked/staged** or the flake source snapshot will not include them.
  If Nix says a module/file cannot be found even though local `npm run build`
  works, check whether the new files are tracked.
- Local checks that should remain green after each slice:
  - `cd apps/admin-ui && npm test && npm run build`
  - `nix build .#underwrap-admin-ui-nextjs --no-link`
  - backend-side endpoint tickets should also run the relevant `cargo test -p ...`
- Next.js currently emits a non-fatal tracing warning because the runtime config
  loader reads files from disk. Local build and Nix build still pass.

## Acceptance criteria

- [ ] A Next.js/TypeScript admin app replaces `crates/backend-admin-ui` as
      the intended operator UI.
- [ ] The browser interacts only with the new app; backend admin bearer
      tokens stay server-side.
- [ ] Username/password auth remains supported using file-backed Argon2
      hashes.
- [ ] Operators can reveal, edit, delete, upload, and download secret
      values through a deliberate flow that avoids URL leakage, handles
      binary data safely, and enforces a 1 MiB per-file limit in v1.
- [ ] Wrapper mode management and access-matrix management are materially
      better UX than the current Rust UI.
- [ ] A read-only, filterable audit/history page exists in the new UI.
- [ ] Backend admin API includes the read/write/audit shapes needed for a
      clean frontend architecture, including explicit audit events for UI
      reveal/download actions.
- [ ] Nix packaging, NixOS integration, and deployment docs exist for the
      new app.
- [ ] End-to-end tests cover login, secret reveal, binary upload/download,
      mutation enable/write, wrapper mode changes, access matrix changes,
      audit history, and CSRF/security rejection paths.
- [ ] The old Rust UI implementation is removed after cutover.
