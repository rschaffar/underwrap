# Story: Cutover, removal of Next.js app, and documentation refresh

Parent: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Background

Once the new SPA reaches feature parity and the multi-user / stores work is green, the Next.js BFF can be deleted and the docs refreshed to match the new architecture.

## Tasks

1. `260_task_remove_nextjs_app_and_nodejs_packaging.md`
2. `261_task_docs_and_security_md_update.md`
3. `262_task_nixos_vm_test_end_to_end.md`

## Acceptance criteria

- [ ] `apps/admin-ui/` removed.
- [ ] `nix/modules/admin-ui-nextjs.nix` removed.
- [ ] `underwrap-admin-ui-nextjs` flake output removed.
- [ ] No `nodejs` in any runtime closure.
- [ ] Docs (CLAUDE.md, README.md, docs/security.md, docs/backend.md) reflect the new architecture.
- [ ] VM test exercises the full end-to-end flow on a real NixOS host.
