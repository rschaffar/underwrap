# Story: Roll out the `backend-backendd` refinement tasks

Parent: `epic_backend_backendd_refinement.md`

## Scope

Land the thirteen task tickets below, in order. Each task is one PR
that:

1. Resolves the listed findings from
   `tickets/_review/backend-backendd_2026-05-21.md`.
2. Keeps `cargo test --workspace`, `cargo fmt -- --check`, and
   `cargo clippy --all-targets --workspace -- -D warnings` green.
3. Does not change observable behaviour, except where the ticket
   explicitly calls out a behaviour change (only 1115 does, and the
   user has signed off).

Tasks are ordered so each one depends only on earlier ones:

1. `1010_task_drop_server_admin_store_shim.md` — unblocks every
   later move by closing the F-001 stop-gap.
2. `1015_task_drop_v1_users_migration.md` — independent greenfield
   cutover (F-013, user decision 2026-05-21).
3. `1020_task_collapse_load_policy_document_helpers.md` — F-005.
4. `1030_task_remove_rotate_key_dead_code.md` — F-003, F-004.
5. `1040_task_extract_random_id_helper.md` — F-006.
6. `1050_task_clean_stale_ticket_comments.md` — F-009.
7. `1060_task_rename_validate_store_name.md` — F-010.
8. `1070_task_split_auth_tests_sibling.md` — F-007.
9. `1080_task_split_users_runtime_clients_web_tests.md` — F-008.
10. `1090_task_consolidate_admin_state_test_builders.md` — F-011.
11. `1100_task_resolve_http_mod_todo.md` — F-012, needs user choice.
12. `1110_task_decide_server_public_api_surface.md` — F-002, needs
    user choice.
13. `1120_task_final_simplification_pass.md` — sweep over every file
    touched in 1010–1110.

The two leaf-fix tasks 1015 and 1020 are independent of 1010 and
could run in parallel; the suggested order just minimises rebase
churn against the import-site changes from 1010.

## Housekeeping prerequisite

Before starting 1010, move:

- `tickets/01_todo/epic_backend_modularization.md` → `tickets/03_done/`
- `tickets/01_todo/700_story_backend_modularization.md` → `tickets/03_done/`

Both describe work that's already in the tree (server.rs is 207
lines, the target layout exists, all `710_…820_` child tasks are
already in `03_done/`). The execution session should do this as a
no-code git move at the top of the loop, then start on 1010.

## Acceptance criteria

- [ ] All thirteen task tickets are done.
- [ ] The modularization epic + story have moved to `03_done/`.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes (verified at
      the end of 1120, and after 1100 if option 1 is chosen).
- [ ] `crates/backend-backendd/src/server.rs` no longer carries the
      `pub(crate) use crate::admin_api::stores::admin_store;` shim
      from line 38-42.
- [ ] No reference to `load_policy_document_or_default` survives.
- [ ] `auth.rs`, `users.rs`, `runtime_clients.rs`, `web.rs` each have
      a sibling `<name>/tests.rs` file and no inline `mod tests` block.
- [ ] The v1 branch in `users.rs::load_state_document` is gone and
      no test seeds an `operators:`-shaped v1 file.

## Out of scope

See the epic.
