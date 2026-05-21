# Task: Extract `admin_api/resources`

Parent: `700_story_backend_modularization.md`

## Scope

Move the resource admin subdomain into its own module. ~420 lines on
its own, shares the `storage::documents::resource_secret_dependents`
helper that moved in 740.

### `crates/backend-backendd/src/admin_api/resources.rs` (new)

Move verbatim from `server.rs`:

- `handle_admin_resources_list` (4863-4888)
- `handle_admin_resource_blob_get` (4890-4924)
- `handle_admin_resource_blob_put` (4926-4996)
- `handle_admin_resource_create` (4998-5065)
- `handle_admin_resource_delete` (5067-5206)
- `handle_admin_resource_rename` (5208-5274)

### `crates/backend-backendd/src/admin_api/mod.rs`

Declare the `resources` submodule. `handle_admin_request` updates the
six `/v1/admin/resources/*` match arms to call the new locations.

## Acceptance criteria

- [ ] `admin_api/resources.rs` exists with the listed contents.
- [ ] `server.rs` no longer defines any of the listed handlers.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.

## Out of scope

- Restructuring the resource blob format or the wire types.
- Combining `resources.rs` with `pairs.rs` even though pairs reference
  resources — they're separately addressable URL surfaces and keeping
  them apart matches the SPA's mental model.
