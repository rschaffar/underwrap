# Story: Harden, test, document, and cut over to the new admin UI

Epic: `tickets/01_todo/epic_nextjs_admin_ui_replatform.md`

## Background

A modern JS UI only helps if it preserves the project’s security posture
and finishes with a clean cutover. This ticket covers the last-mile work
that makes the new UI real rather than experimental.

## Scope

- Add end-to-end browser tests for the primary operator flows.
- Add security regression tests for session/auth/CSRF and secret reveal
  handling.
- Set a production CSP strategy appropriate for a Next.js app.
- Ensure no backend bearer tokens or secret values are logged by the UI.
- Document deployment behind a TLS-terminating reverse proxy.
- Update the admin UI docs to match the new implementation and UX.
- Remove the old Rust UI implementation after parity/signoff.

## Acceptance criteria

- [ ] End-to-end tests cover login, resource reveal, mutation write,
      wrapper mode edit, and access-matrix edit.
- [ ] The new UI has an explicit CSP and documented deployment shape.
- [ ] The new UI does not persist secrets in browser storage.
- [ ] Docs under `docs/` describe the new app rather than the removed
      Rust HTML UI.
- [ ] `crates/backend-admin-ui` is removed once the new app is ready.
- [ ] The repo has one clear admin UI path, not two competing ones.

## Child tasks

- `120_task_admin_ui_hardening_playwright_and_secret_leak_regressions.md`
- `130_task_admin_ui_docs_cutover_and_remove_rust_ui.md`
