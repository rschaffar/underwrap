# Story: Roll out the 2026-05-22 `web/` admin SPA refinement tasks

Parent: `epic_web_spa_refinement_2026-05-22.md`

## Scope

Land the 13 task tickets below, in the order shown. Each task is
one PR that:

1. Resolves the listed findings from
   `tickets/_review/web_spa_2026-05-22.md`.
2. Keeps `cd web && npm test` and `cd web && npm run build` green.
3. Keeps `cd web && npm run test:e2e` green for any ticket that
   touches a page module or a component used by Playwright.
4. For 1570 only: also keeps `cargo test --workspace`,
   `cargo fmt -- --check`, and
   `cargo clippy --all-targets --workspace -- -D warnings` green.
5. Does not change observable behaviour. The lone cross-crate
   touch (1570) flips a ts-rs annotation; the JSON wire shape
   is identical.

Tasks are ordered so each one depends only on earlier ones:

1. `1510_task_extract_shared_lib_helpers.md` — F-012, F-018,
   F-028, F-032. Lib helpers (epoch, creator, datetime-local).
2. `1520_task_extract_use_store_selector_and_lookup_hooks.md` —
   F-017, F-019, F-027.
3. `1530_task_extract_alert_components.md` — F-021. New
   `<Alert>` primitive replaces 25+ inline divs.
4. `1540_task_add_select_primitive.md` — F-020. New `<Select>`
   primitive replaces 6+ inline styled selects.
5. `1550_task_collapse_api_client_type_reexports.md` — F-006
   (partial), F-007. Drop 140 LoC of import-then-reexport.
6. `1560_task_drop_unused_principal_props_and_route_wrappers.md`
   — F-010, F-024, F-033. Four pages and six route wrappers
   collapse.
7. `1570_task_clean_wrapper_approval_mode_typing.md` — F-016,
   F-026. Tiny Rust annotation tweak + SPA cast cleanup.
8. `1580_task_split_stores_page_into_files.md` — F-003.
9. `1590_task_split_secrets_page_into_files.md` — F-001.
10. `1600_task_split_wrapper_permissions_page_into_files.md` —
    F-002. Benefits from 1570 having landed.
11. `1610_task_split_client_instances_page_into_files.md` — F-004.
12. `1620_task_split_users_page_into_files.md` — F-005.
13. `1630_task_web_misc_dead_code_and_stale_tests.md` — F-008,
    F-009, F-011, F-013, F-023 (decision branch), F-029, F-030,
    F-031.
14. `1640_task_repo_sweep_drop_ticket_references_in_code_and_docs.md`
    — F-014, F-015. Project-wide sweep; independent of the
    others.

The page splits (1580–1620) are independent of each other and
could land in parallel; the suggested order is shortest-first to
build confidence in the split pattern before tackling the two
1000+ LoC monsters.

## Acceptance criteria

- [ ] All 13 task tickets (1510–1640) are in `03_done/`.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes (Playwright).
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` and
      `cargo clippy --all-targets --workspace -- -D warnings` pass.
- [ ] `rg -n 'as WrapperApprovalMode|as unknown as WrapperApprovalMode' web/src/`
      returns nothing.
- [ ] `rg -n 'role="alert" className="rounded-md border border-rose-500' web/src/`
      returns nothing (every site routed through `<Alert>`).
- [ ] No `pages/*.tsx` file at the leaf level exceeds 600 LoC.
- [ ] No `principal?:` declaration survives in the four pages
      that don't read it.
- [ ] The repo-wide ticket-reference grep
      (`rg -n -i '(ticket[[:space:]]*[0-9]{3,4}|epic_[a-z_]+|story_[a-z_]+|story \d+|stories \d+|TODO\([0-9]{3,4}\)|FIXME\([0-9]{3,4}\))' web/src/ docs/ examples/ crates/ nix/`)
      returns nothing.

## Out of scope

See the epic.
