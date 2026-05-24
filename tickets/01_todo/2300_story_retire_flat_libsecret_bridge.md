# Story: Retire the flat libsecret bridge

## Background

See `epic_libsecret_structured_admin.md`. After
`2200_story_libsecret_structured_admin_api.md` lands and the admin SPA
has switched to the structured endpoints (`2400`), the libsecret
branches of the flat KV bridge in `shared-store/src/resource_blob.rs`
have no remaining consumer:

- Runtime IPC (daemon ↔ backend, daemon ↔ secretsd) already exchanges
  the structured `ResourceBlob` / `LibsecretSection` bytes.
- The admin API now has structured libsecret endpoints.
- Only `env` and `files` slots still need the flat-KV admin shape, and
  they genuinely are key/value.

This story removes the dead libsecret cases from the bridge and
narrows the admin's flat-secret endpoint to env + files only.

This story must **not** land until the SPA has stopped calling the flat
libsecret endpoints — otherwise the SPA breaks.

## Goal

Make the synthetic flat libsecret keys (`_aliases`, `<col>/_meta`,
`<item>.meta`, `<item>.secret`) impossible to construct anywhere in the
codebase. The flat bridge survives only for `env` and `files`.

## Scope

### 1. Strip libsecret from the bridge

In `crates/shared-store/src/resource_blob.rs`:

- Drop `libsecret_section_to_kv` and `libsecret_section_from_kv`
  entirely.
- Drop the libsecret branches from `expand_blob_to_entries` and
  `blob_from_resource_entries`. They become small env/files-only
  functions.
- Remove the now-dead wire structs (`WireCollectionMeta` /
  `WireCollectionMetaOwned` / `WireItemMeta` / `WireItemMetaOwned`).
- Update tests in `resource_blob/tests.rs`: remove all libsecret
  round-trip / orphan-scrub cases. Keep env / files cases.

### 2. Narrow the slot enum at the admin boundary

In `shared-protocol::resource`:

- `parse_secret_id` and `compose_secret_id` should reject the
  `libsecret` slot when called from admin endpoints. Either introduce a
  narrower `AdminFlatSlot { Env, Files }` enum used only by admin
  endpoints, or have `parse_secret_id` return an error and update
  `validate_and_compose_secret_id` accordingly.
- `ResourceSlot::Libsecret` may remain in the underlying enum if it is
  still referenced elsewhere (e.g. audit logging), but it is no longer
  reachable through the flat admin path.

### 3. Drop libsecret from the flat admin endpoints

In `crates/backend-backendd/src/admin_api/secrets.rs`:

- `parse_slot` rejects `"libsecret"` with a clear error message
  pointing operators at the new structured endpoints.
- `handle_admin_secrets_list` filters out libsecret entries — and after
  step 1 there are no libsecret entries to filter; the call simply
  produces env + files.
- Audit events with `slot=libsecret` are no longer reachable here.

### 4. Update integration tests

- Remove any flat-libsecret coverage from
  `crates/integration-tests/src/tests/` (was added pre-rework).
- Keep flat env / files coverage as-is.
- Confirm the structured libsecret integration tests from `2200` still
  pass.

### 5. Documentation

- `docs/backend.md` — replace any mention of `_aliases`, `<col>/_meta`,
  `<item>.meta`, `<item>.secret` with pointers to the structured
  endpoints.
- `crates/backend-backendd/src/admin_api/secrets.rs` module-level
  docstring — note that this module now serves only env / files.

## Out of scope

- Changing the `ResourceBlob` schema.
- Changing the daemon ↔ secretsd / daemon ↔ backend runtime paths.
- Any SPA work (that is `2400`).

## Acceptance criteria

- [ ] `expand_blob_to_entries` and `blob_from_resource_entries` only
      handle env and files. `grep '_aliases' --include='*.rs'` returns
      no hits.
- [ ] The flat admin secret endpoints reject `slot=libsecret` with a
      structured `slot_not_supported` error pointing at the new
      endpoints.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo fmt -- --check` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.
- [ ] Documentation updated.

## Notes for the implementer

- This story is destructive in a small way. Confirm the SPA is on the
  structured endpoints (`2400` merged) before landing.
- The 2120 quickfix (`_aliases` re-emission) becomes unreachable code
  after step 1 and is deleted naturally.
