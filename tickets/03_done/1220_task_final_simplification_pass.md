# Task: Final simplification pass

Parent: `1130_story_admin_api_refinement.md`
Resolves: trailing simplification across files touched in 1140-1210.

## Background

The skill that produced this epic calls for one closing pass that
re-reads every file touched by the prior tasks against the category 7a
checklist from `CLAUDE.md` and removes anything that still fits:

- needless abstractions or wrapper types with a single caller
- defensive branches that can't fire (trust internal code; validate
  only at system boundaries)
- redundant clones, allocations, or `to_owned()` calls
- layers of indirection added for hypothetical future requirements
- hand-rolled logic where a short stdlib / iterator-combinator call
  would do
- options, configs, or feature flags that are never set to anything
  but the default
- error-handling code paths for impossible states
- re-implementations of patterns already present elsewhere

This is **not** a place to start new architectural rework. If a
finding under this lens is non-trivial, file a follow-up ticket and
leave the code alone here.

## Scope

Re-read every file touched in 1140-1210:

- All admin_api modules touched by 1140 (essentially every file in
  `admin_api/` except `audit.rs`, `csrf.rs`, `mod.rs`, `overview.rs`).
- `client_instances.rs` (1150).
- `policy.rs` (1160).
- `ids.rs` + `stores/`, `client_instances.rs` (1170).
- `stores/` (1180, 1200).
- All allow-stripped sites (1190).
- `shared-store/src/plain.rs` + `tests.rs`, `resource_blob.rs` +
  `tests.rs`, `frontend-underwrapd/src/wrapper_bindings.rs` +
  `tests.rs` (1210).

For each file, apply the checklist above. Land what's a clear win;
park what isn't.

Acceptance gates include the VM test, since the cumulative diff
across the epic is wide enough that the unit suite alone is no
longer the only safety net.

## Out of scope

- New behaviour changes.
- New ticket creation for findings inside this pass — only do that
  if a finding is substantial. Small wins land in this PR.
- Audit-schema unification (F-009): still routed out as a user-owned
  RFC; do not touch.

## Acceptance criteria

- [ ] Every file from the list above has been re-read end-to-end.
- [ ] Any simplification found has either been applied in this PR or
      filed as a follow-up ticket (with a one-line note in the
      commit message naming the follow-up).
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.
