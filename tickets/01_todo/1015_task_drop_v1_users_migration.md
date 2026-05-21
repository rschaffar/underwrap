# Task: Drop the v1 `users.json` migration path

Parent: `1000_story_backend_backendd_refinement.md`
Resolves: F-013 (review report `backend-backendd_2026-05-21.md`)

## Background

`crates/backend-backendd/src/users.rs` still accepts the v1
`operators.json` layout (`operators:` key, per-record `role` and
`granted_stores` fields) and silently rewrites it as v2 on the next
save. The user has decided (2026-05-21) that the project's greenfield
posture wins here: no parallel migration code, new schema only.

Deleting the migration also drops the v1 test fixture
(`load_v1_state_file_migrates_to_v2_dropping_role_and_grants`) and
the version-routing branch in `load_state_document`.

This is the only task in the epic that changes observable behaviour:
loading a v1 file now fails fast with an "unsupported version" error
instead of being silently migrated. That matches every other
versioned document in the crate (`StoreRegistryDocument`,
`ClientInstancesDocument`, `ResourcesDocument`, `PairsDocument` — all
reject non-current versions).

## Scope

In `crates/backend-backendd/src/users.rs`:

- Delete `parse_v1_document` (lines ~325-363).
- In `load_state_document` (lines ~293-319), drop the `match version`
  arms and just attempt to deserialise the current `UserStateDocument`
  shape; rely on the existing `STATE_FILE_VERSION` check that
  `save_state_document` already performs.
- Simplify the error message on version mismatch to match the wording
  used in `runtime_clients.rs` / `store_registry.rs` / the entities
  documents (`"unsupported user-state file version {n}"`).

In the inline `mod tests` (lines ~397-605):

- Remove `load_v1_state_file_migrates_to_v2_dropping_role_and_grants`.
- If a v1-rejection coverage test feels right, add a tiny one
  asserting that a v1-shaped file produces the "unsupported version"
  error. Keep it under ~15 lines; this is not the place to invent a
  new fixture matrix.

In `docs/backend.md` and any neighbouring docs that named the v1
shape: skim for references and remove only those that directly named
the migration. Don't broaden into a doc pass.

## Out of scope

- Splitting `users.rs` into a sibling tests file. Ticket 1080 does
  that.
- Touching the v1 schema fields (`role`, `granted_stores`)
  elsewhere — they only existed inside the migration helper.

## Acceptance criteria

- [ ] `grep -n "parse_v1_document\|operators\":" crates/backend-backendd/src/users.rs`
      returns no matches.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
- [ ] Loading a v1-shaped JSON file produces a clear
      "unsupported user-state file version" error (covered by a
      small new test, optional but recommended).
