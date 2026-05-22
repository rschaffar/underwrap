# Story: Roll out the `admin_api` refinement tasks

Parent: `epic_admin_api_refinement.md`

## Scope

Land the nine task tickets below, in order. Each task is one PR that:

1. Resolves the listed findings from
   `tickets/_review/admin_api_2026-05-22.md`.
2. Keeps `cargo test --workspace`, `cargo fmt -- --check`, and
   `cargo clippy --all-targets --workspace -- -D warnings` green.
3. Does not change observable behaviour, except where the ticket
   explicitly calls out a behaviour change (only 1150 does, and the
   user has signed off).

Tasks are ordered so each one depends only on earlier ones:

1. `1140_task_unify_body_handler_split_pattern.md` — F-003, F-004.
   Renaming sweep across `users.rs`, `client_instances.rs`,
   `stores.rs`, `pairs.rs`, `policy.rs`, `resources.rs`,
   `secrets.rs`, `auth.rs`. First so subsequent tickets don't
   rebase through naming churn.
2. `1150_task_client_instance_update_cascade_log_and_continue.md` —
   F-010. Behaviour change; runs directly after the rename sweep so
   it lands on the post-rename shape as a clean one-PR diff.
3. `1160_task_policy_put_use_save_helper.md` — F-005.
4. `1170_task_extract_fresh_id_helper.md` — F-006.
5. `1180_task_dedupe_mapped_client_instance_count.md` — F-007.
6. `1190_task_drop_dead_result_large_err_allows.md` — F-008.
7. `1200_task_split_stores_module.md` — F-001, F-002. Last among the
   in-component work because it's the biggest move; the post-leaf
   shape travels with it.
8. `1210_task_repo_sweep_split_inline_tests.md` — F-011. Repo-wide
   sweep, independent of everything else.
9. `1220_task_final_simplification_pass.md` — sweep over every file
   touched in 1140–1210.

Leaf fixes 1160 / 1170 / 1180 / 1190 are independent of each other;
the suggested order is arbitrary. 1210 leaves the `admin_api`
context and could run at any point; it's last because the
simplification sweep is easier to scope when 1140–1200 have all
landed.

## Acceptance criteria

- [ ] All nine task tickets are done.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes (verified at
      the end of 1220).
- [ ] No `do_admin_*` function survives in `crates/backend-backendd/src/admin_api/`.
- [ ] `handle_admin_policy_put` uses `save_policy_document`.
- [ ] `crates/backend-backendd/src/admin_api/stores.rs` no longer
      exists as a single file; the `stores/` submodule has at least
      `mod.rs`, `crud.rs`, `lifecycle.rs` and matching test siblings.
- [ ] `crates/shared-store/src/plain.rs`,
      `crates/shared-store/src/resource_blob.rs`, and
      `crates/frontend-underwrapd/src/wrapper_bindings.rs` each have
      a sibling `<name>/tests.rs` file and no inline `mod tests`
      block.

## Out of scope

See the epic.
