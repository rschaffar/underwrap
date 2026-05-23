# Epic: Web admin SPA refinement

Fold the `web/src` UI codebase into one consistent shape ahead of
first release. Source review:
`tickets/_review/web_admin_spa_2026-05-23.md`.

Three idioms (page-level error rendering, document-title setters,
confirm-dialog scaffolding) get collapsed into small shared helpers.
Three dialogs that bypass the `Form`/`FormField` pattern adopt it.
Two dead exports and one stale comment block are removed. The
aggregate-view cache key on the policy viewer is brought in line
with the rest of the SPA.

Two cross-cutting cleanups land in the same epic:

- **Operator-name rejections go away** (2070). Resource ids, store
  ids, user ids, and client-instance ids are all generated
  server-side. Operator-supplied names are pure display labels and
  do not need character-set or "reserved prefix" gating. The
  per-runtime gates that protect *real* artifacts (env-var names,
  file-path components) stay.
- **Password length rule moves to the backend** (2080). One
  authoritative `MIN_PASSWORD_LEN` enforced in the handlers; the
  SPA drops its `.min(N)` and lets the backend's stable
  `password_too_short` code drive the message.

One project-wide sweep ticket (2100) relocates the remaining
inline `mod tests {}` blocks in Rust crates to sibling `tests.rs`
files, matching the now-unconditional AGENTS.md rule.

## Tickets

- `2000_story_web_admin_spa_refinement.md` — story / container
- `2010_task_unify_inline_error_displays_and_document_title.md`
- `2020_task_extract_confirm_dialog_component.md`
- `2030_task_adopt_form_pattern_in_remaining_dialogs.md`
- `2040_task_remove_dead_modekind_and_derive_client_instances.md`
- `2050_task_simplify_secrets_and_provenance_helpers.md`
- `2060_task_tidy_stale_comments_and_naming_in_api_client_and_pages.md`
- `2070_task_remove_name_based_rejections.md`
- `2080_task_enforce_password_min_length_in_backend.md`
- `2090_task_self_invalidating_dialog_pattern.md`
- `2100_task_repo_sweep_relocate_all_inline_tests.md`

## Sequencing notes

- 2010 and 2020 introduce shared helpers; later tasks consume
  them, so land these first.
- 2030 (Form-pattern adoption) is independent of 2010/2020 but
  benefits from the `<Alert>`-everywhere convention; do it after.
- 2080 (password rule cutover) touches the wire surface. Land
  before the SPA forms drop their `.min(N)` so the user can't end
  up with a window where both sides reject differently.
- 2100 (inline-test relocation) is mechanical and can land at any
  time; do it last to avoid a churn collision with the SPA work.

## Acceptance criteria for the epic

- All ten task tickets in `tickets/03_done/`.
- `cargo test --workspace`,
  `cargo clippy --all-targets --workspace -- -D warnings`,
  `cargo fmt -- --check`,
  `cd web && npm test`,
  `cd web && npm run build`,
  `cd web && npm run test:e2e`,
  `nix build .#checks.x86_64-linux.vm-test` all pass.
