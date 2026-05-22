# Epic: Second-pass refinement of `backend-backendd` (2026-05-22)

## Background

`epic_backend_backendd_refinement` (closed) and `epic_admin_api_refinement`
(closed) cleaned up the crate roots and the `admin_api/` subtree. A fresh
2026-05-22 review of the now-separated crate found 20 more findings across
8 categories — mostly pre-first-release schema-version baggage on every
persisted document, `_state`/`_now_epoch`/`_stores` audit-shim parameters
that nothing reads, brittle `msg.contains(...)` error-reason sniffing
across user CRUD plus one runtime path, the `_endpoint`/inner-handler
ceremony that survives even where no test calls the inner form, and
duplication between two id validators + four hand-rolled hex encoders.
Plus two repo-wide sweeps that the previous epic's ticket 1210 only
covered for three files: the rest of the workspace still carries
over-threshold inline `mod tests {}` blocks and 32 ticket / epic / story
pointers in source/config/docs.

The full review is at
`tickets/_review/backend_backendd_2026-05-22.md`. Every task ticket
below names the finding IDs it resolves so the report can be audited
against the work once each task lands.

## Goals

1. **Strip every `version: u32` discriminator + matching `serde(default)`
   from the documents this crate persists.** The first release is
   version 1, full stop. No deployed shapes to migrate from.
2. **Replace `msg.contains(...)` error-reason sniffing in user CRUD
   with a typed `UserMutationError` enum.** The four user handlers +
   the password-change handler match on the variant instead of
   stringly-typed fallthroughs. (The matching runtime-side
   `schema_version` sniff stays out — it needs a `shared-store`
   change routed to a separate non-refactor ticket.)
3. **Drop the unused `_state`, `_now_epoch`, `_stores` parameters and
   their callers' computations.** Four audit helpers, one overview
   helper, one test fixture.
4. **Collapse the `_endpoint`/inner-handler pattern where no test
   calls the inner form.** Keep the split only where unit tests
   genuinely drive the body-taker.
5. **Extract the session-issuing dance into one helper.** Login and
   bootstrap stop carrying a 30-line copy each.
6. **Extract the open/finalize policy-lookup → deny-response
   translation in `runtime_api/invocations.rs`.** Both handlers
   collapse a 50-line match block to one call.
7. **Consolidate id and time helpers.** One `is_valid_simple_id` in
   `crate::ids`; `hex::encode` everywhere; `users::now_epoch_secs()`
   everywhere.
8. **Sweep the remaining doc/dep hygiene.** Stale `Cargo.toml`
   description, duplicate `tempfile` dev-dep, missing module doc on
   `runtime_clients.rs`, `epic_*` pointer in `runtime_state.rs`,
   "Next.js BFF" remnants in `auth.rs`, legacy-inline-config
   rejection in `main.rs`, `whoami` silent error swallow.
9. **Repo-wide sweep: relocate every over-threshold inline `mod tests {}`
   block to a sibling `tests.rs`.** Ticket 1210 covered three files;
   ~27 more remain across `shared-protocol`, `shared-store`,
   `frontend-*`, and `integration-tests`. Plus the `main.rs` in this
   crate (which needs the move-into-library trick).
10. **Repo-wide sweep: strip every ticket / epic / story pointer out
    of source/config/docs.** 32 occurrences listed in the review.

## Stories

- `1300_story_backend_backendd_refinement.md` — single rollout story
  enumerating the task tickets.

## Product decisions

- **No behaviour change.** Every task except F-014's `whoami` warn
  is a pure refactor. F-014 only adds a `warn!` log line, no wire
  shape change. The user has not been asked to sign off on a
  behaviour flip because there isn't one.
- **One PR per task.** Each task ticket leaves the tree green
  (`cargo test --workspace`, `cargo fmt -- --check`,
  `cargo clippy --all-targets --workspace -- -D warnings`).
- **VM test gates only the schema-version removal (1310), the final
  hygiene sweep (1390), and the two repo-wide sweeps (1400, 1410).**
  Every other task is crate-local enough that the unit suite catches
  breakage.
- **Schema-version removal lands first (1310).** It changes serde
  shapes for every persisted document; later refactors otherwise have
  to re-touch the same structs.
- **Repo-wide sweeps land last.** They apply to a stable
  post-refactor codebase.

## Out of scope

- **Typed `BlobParseError` in `shared-store`.** Needed for F-005's
  runtime-side `schema_version` sniff; lives outside this crate.
  Owner: user, separate non-refactor ticket.
- **General WHAT-comment sweep.** Skill posture's comment rule
  appears mostly respected in this crate; no systemic violation
  worth a sweep ticket inside this component.
- **Wire-format, NixOS-module, or config-schema changes.** None of
  the tasks below need them.

## Review trail

Findings live at
`tickets/_review/backend_backendd_2026-05-22.md`. Every task ticket
below names the finding IDs it resolves so the report can be audited
against the work once each task lands.
