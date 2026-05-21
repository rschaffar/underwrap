# Epic: Refine `backend-backendd` after the modularization epic

## Background

The post-`epic_backend_modularization` shape of
`crates/backend-backendd/` is clean structurally — the giant
`server.rs` is gone, the per-subdomain `admin_api/` modules each own
one URL family, and the storage/HTTP helpers live in the modules the
epic targeted. The review at
`tickets/_review/backend-backendd_2026-05-21.md` confirmed that and
turned up thirteen smaller findings: stop-gap re-exports that the
epic left behind, dead code in rotate-key, duplicated helpers
(`load_policy_document` and `_or_default` are the same function,
three near-identical id generators), the per-module tests that
weren't split to the sibling-file pattern, stale ticket-number
comments referring to work that's done, and one cross-cutting
greenfield cutover (drop the v1 `users.rs` migration path now that
the project is committed to "new shape only").

This epic finishes the cleanup the modularization epic explicitly
identified as follow-up plus a handful of nearby simplifications, and
locks the v1 migration off so the user-state loader can stop carrying
parallel code paths.

## Goals

1. The `crate::server::admin_store` re-export shim is gone and every
   admin module imports the helper directly from
   `admin_api::stores`.
2. The two `load_policy_document(_or_default)` helpers collapse to
   one, and every caller (including the inline reinvention in
   `policy.rs::handle_admin_policy_get`) uses it.
3. The three random-id generators (store, client-instance, resource)
   share one helper.
4. The dead `installed_key` placeholder in `rotate-key` is removed
   and `stage_name` no longer abuses the master-key helpers for a
   random suffix.
5. `auth.rs`, `users.rs`, `runtime_clients.rs`, and `web.rs` follow
   the sibling-file test pattern that `CLAUDE.md` mandates and that
   every other module in the crate already uses.
6. `test_support.rs` exposes one `default_admin_runtime_state()`
   helper that the five test-state builders call into instead of
   restating every field. The file moves from 753 lines toward
   ~500.
7. Stale ticket-number comments (`(ticket 212)` etc.) and "until
   then" footnotes are gone; load-bearing reasons replace them.
8. `validate_store_name` no longer doubles as a generic display-name
   validator, or it is renamed/relocated to honestly describe what
   it does.
9. The `http/mod.rs` TODO is resolved one way or the other (move the
   transport wiring out of `server.rs`, or drop the TODO).
10. The public-API surface (`underwrap_backendd::server::*` vs.
    `underwrap_backendd::*`) is consciously chosen, not maintained
    by accident.
11. The v1 `users.rs` migration path is removed; new on-disk files
    use the current schema only.
12. After the mechanical work lands, the touched files are re-read
    against the simplification checklist and any remaining low-hanging
    fruit (defensive branches that can't fire, redundant clones,
    single-caller wrapper types, etc.) is removed.

## Stories

- `1000_story_backend_backendd_refinement.md` — single rollout story
  enumerating the task tickets.

## Product decisions

- **No behavioural change in the refactor tasks.** The only ticket
  that flips observable behaviour is 1115 (drop v1 users.json
  migration), and the user has explicitly signed off on it: the
  greenfield posture wins, no parallel migration code, new schema
  only.
- **One PR per task.** Same convention as the modularization epic.
  Each task ticket leaves the tree green
  (`cargo test --workspace`, `cargo fmt -- --check`,
  `cargo clippy --all-targets --workspace -- -D warnings`).
- **VM test gates only the transport-touching task** (1100 option 1)
  and the final simplification sweep (1120). Everything else is
  crate-local enough that the unit suite catches breakage.
- **Two tasks need a user decision** before they start: 1100
  (move transport wiring to `http/mod.rs` or drop the TODO) and 1110
  (lift re-exports to `lib.rs` or delete the misleading "until then"
  comment in `server.rs`). The task tickets call this out; do not
  pick a side without checking in.
- **Housekeeping prerequisite:** the modularization epic in
  `tickets/01_todo/` describes work that is now in the tree.
  `epic_backend_modularization.md` and
  `700_story_backend_modularization.md` should move to `03_done/`
  before the new epic starts, so the board reflects reality. The
  story's individual task tickets (`710_…820_`) are already in
  `03_done/`.

## Out of scope

- Splitting `AdminRuntimeState` into per-concern structs. The
  modularization epic deferred this and the deferral still holds.
- Audit-log schema unification across the daemon. Two distinct
  shapes (`admin_api/audit.rs::log_admin_event` vs. inline
  `audit = "secret_…"` lines) exist; reconciling them deserves its
  own cross-component RFC.
- Any new wire-format, NixOS-module, or config-schema change.
- Documentation rewrites in `docs/*.md`. If a task moves code, it
  updates only the `docs/*.md` lines that named the moved symbol;
  no broader doc pass.

## Review trail

Findings live at
`tickets/_review/backend-backendd_2026-05-21.md`. Every task ticket
below names the finding IDs it resolves so the report can be
audited against the work once each task lands.
