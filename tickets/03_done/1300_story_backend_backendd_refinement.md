# Story: Roll out the 2026-05-22 `backend-backendd` refinement tasks

Parent: `epic_backend_backendd_refinement_2026-05-22.md`

## Scope

Land the ten task tickets below, in order. Each task is one PR that:

1. Resolves the listed findings from
   `tickets/_review/backend_backendd_2026-05-22.md`.
2. Keeps `cargo test --workspace`, `cargo fmt -- --check`, and
   `cargo clippy --all-targets --workspace -- -D warnings` green.
3. Does not change observable behaviour, except where the ticket
   explicitly calls one out (only 1390's F-014 sub-bullet adds a
   `warn!` log line — no wire-shape change).

Tasks are ordered so each one depends only on earlier ones:

1. `1310_task_drop_pre_first_release_schema_versioning.md` — F-001.
   Lands first because it changes serde shapes that other tasks
   would otherwise re-touch.
2. `1320_task_typed_user_mutation_error.md` — F-005 (user-CRUD half).
3. `1330_task_drop_dead_audit_state_param.md` — F-002.
4. `1340_task_drop_dead_overview_now_epoch.md` — F-003.
5. `1350_task_collapse_endpoint_wrappers.md` — F-006.
6. `1360_task_extract_session_issue_helper.md` — F-012.
7. `1370_task_unify_invocation_lookup_helper.md` — F-013.
8. `1380_task_consolidate_id_helpers_and_validators.md` — F-009,
   F-010, F-011.
9. `1390_task_misc_doc_and_dep_hygiene.md` — F-004, F-007, F-008,
   F-014, F-015, F-016, F-017, F-020.
10. `1400_task_repo_sweep_relocate_inline_tests.md` — F-018,
    project-wide.
11. `1410_task_repo_sweep_strip_ticket_references.md` — F-019,
    project-wide.

Several mid-stack tasks (1330, 1340, 1390) are independent of each
other and could run in parallel; the suggested order minimises
rebase churn against `admin_api/` import-site changes.

## Acceptance criteria

- [ ] All ten task tickets are in `03_done/`.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes (verified at
      least at the end of 1310, 1390, 1400, 1410).
- [ ] No `version: u32` discriminator survives on
      `UserStateDocument`, `ClientInstancesDocument`,
      `StoreRegistryDocument`, `ResourcesDocument`, `PairsDocument`.
- [ ] No `msg.contains("not_found" | "already exists" |
      "last_user_protected")` survives in `admin_api/users.rs` or
      `admin_api/auth.rs`.
- [ ] No `_state: &AdminRuntimeState` / `_now_epoch: u64` /
      `_stores: &[&str]` parameter survives anywhere in the crate.
- [ ] `rg -n -i '(ticket[[:space:]]*[0-9]{3,4}|epic_[a-z_]+|story_[a-z_]+|TODO\([0-9]{3,4}\)|FIXME\([0-9]{3,4}\))' crates/ web/ nix/ docs/ examples/`
      returns nothing (excluding the `tickets/` tree itself).
- [ ] No inline `#[cfg(test)] mod tests { ... }` block longer than
      ~30 lines survives outside the sub-threshold list in F-018.

## Out of scope

See the epic.
