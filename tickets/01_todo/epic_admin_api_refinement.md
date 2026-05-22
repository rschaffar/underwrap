# Epic: Refine the `backend-backendd::admin_api` subtree

## Background

`epic_backend_backendd_refinement` (closed in commit `6c1fcea`) cleaned
up the crate roots but stopped at the `admin_api/` boundary, where most
admin URL families live. The follow-up review at
`tickets/_review/admin_api_2026-05-22.md` examined those 13 modules
plus their sibling tests in detail and turned up 11 findings: three
different patterns for "split body parsing from the handler"; a
double `admin_store()` lookup in two secret handlers; `handle_admin_policy_put`
hand-rolling a write the rest of the file does through
`save_policy_document`; two near-identical id-collision loops; a
`mapped_client_instance_count` projection duplicated between the
locked and unlocked usage paths; ~25–30 dead
`#[allow(clippy::result_large_err)]` attributes; `stores.rs` and its
tests both over the 600-line ceiling; an inconsistent inline-vs-sibling
test layout across three other crates; and one behaviour bug — the
client-instance update cascade runs *after* the on-disk commit and
returns 500 on cascade failure even though the update has already
persisted.

This epic finishes the cleanup on `admin_api/` and folds in the one
behaviour fix the user explicitly asked to land as part of the same
epic (one isolated commit, log-and-continue precedent from the delete
path).

## Goals

1. One pattern for splitting body parsing from the handler. Every
   place that needs both ends becomes `handle_admin_X_endpoint(request)`
   (parse + delegate) and `handle_admin_X(state, principal, &body)`
   (unit-testable body-taker). Every `do_admin_X` rename to
   `handle_admin_X`.
2. The double `admin_store()` lookup in `handle_admin_secret_write_endpoint`
   and `handle_admin_secret_rename_endpoint` collapses naturally as
   the `*_endpoint` wrapper becomes literally `parse_json_body` +
   delegate.
3. The client-instance update cascade matches the delete path: log
   the cascade failure and return success, since the on-disk commit
   has already happened. A unit test covers the failure path.
4. `handle_admin_policy_put` uses `save_policy_document` like every
   other writer in `policy.rs`.
5. The two id-collision loops in `do_admin_store_create` and
   `do_admin_client_instance_create` share one `crate::ids::fresh_id`
   helper.
6. The duplicated `mapped_client_instance_count` projection in
   `admin_store_usage` and `admin_store_usage_for_locked` collapses
   into one helper.
7. The ~25–30 dead `#[allow(clippy::result_large_err)]` attributes
   on non-`Result`-returning functions are gone; only the (small)
   set the lint genuinely flags survives.
8. `stores.rs` (1289 lines) splits into `stores/{mod,crud,lifecycle}.rs`,
   each well under the 600-line ceiling. The 747-line `stores/tests.rs`
   splits alongside.
9. Three other crates with inline `mod tests {}` blocks that exceed
   their production code (`shared-store/{plain,resource_blob}.rs`,
   `frontend-underwrapd/wrapper_bindings.rs`) move to the sibling-file
   pattern that `CLAUDE.md` mandates.
10. After the mechanical work lands, the touched files are re-read
    against the simplification checklist and any remaining
    low-hanging fruit (defensive branches that can't fire, redundant
    clones, single-caller wrappers, etc.) is removed.

## Stories

- `1130_story_admin_api_refinement.md` — single rollout story enumerating
  the task tickets.

## Product decisions

- **One behaviour change in this epic.** 1150 (client-instance update
  cascade) flips observable behaviour from `500 internal` on cascade
  failure to `200 OK` with a `tracing::warn!` and no wire-shape
  change. The user signed off 2026-05-22: match the delete path's
  log-and-continue precedent. Every other task is pure refactor.
- **One PR per task.** Each task ticket leaves the tree green
  (`cargo test --workspace`, `cargo fmt -- --check`,
  `cargo clippy --all-targets --workspace -- -D warnings`).
- **VM test gates only the final simplification sweep (1220).**
  Every other task is crate-local enough that the unit suite catches
  breakage.
- **F-003 decision:** the user chose option 1 — `*_endpoint` +
  body-taking sibling. The body-taker drops the `_endpoint` suffix
  and is what unit tests reach for. The body-taker takes `&body`
  (borrow) to match the existing `pairs.rs` / `resources.rs` shape.
- **F-010 decision:** folded into the refactor epic as its own task
  (1150) so the behaviour change is one reviewable commit, not
  bundled with the F-003 naming sweep.
- **F-001 decision:** split `stores.rs` three ways
  (`mod.rs` / `crud.rs` / `lifecycle.rs`).

## Out of scope

- Audit-event schema unification (F-009 from the review). The
  previous review already routed it out as a cross-component RFC
  the user owns; that routing still holds. The in-component
  helper-vs-inline question is gated on the schema decision.
- Any new wire-format, NixOS-module, or config-schema change.
- Documentation rewrites in `docs/*.md`.

## Review trail

Findings live at `tickets/_review/admin_api_2026-05-22.md`. Every
task ticket below names the finding IDs it resolves so the report
can be audited against the work once each task lands.
