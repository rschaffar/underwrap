# Task: Extract `admin_api/client_instances`

Parent: `700_story_backend_modularization.md`

## Scope

Move the client-instance admin subdomain into its own module. ~460
lines.

### `crates/backend-backendd/src/admin_api/client_instances.rs` (new)

Move verbatim from `server.rs`:

- `client_instance_status_to_wire` (4314-4323)
- `client_instance_status_from_wire` (4325-4334)
- `admin_client_instance_actions` (4336-4349)
- `current_store_names` (4351-4360)
- `admin_client_instance_list_item` (4362-4378)
- `admin_client_instance_detail` (4380-4397)
- `handle_admin_client_instances_list` (4399-4419)
- `handle_admin_client_instance_detail` (4421-4448)
- `handle_admin_client_instance_create` (4450-4460) +
  `do_admin_client_instance_create` (4462-4571)
- `handle_admin_client_instance_update` (4573-4584) +
  `do_admin_client_instance_update` (4586-4713)
- `handle_admin_client_instance_delete` (4715-4774)
- `generate_client_instance_id` (562-570)

### `crates/backend-backendd/src/admin_api/mod.rs`

Declare the `client_instances` submodule. `handle_admin_request`
updates the `/v1/admin/client-instances*` match arms.

## Acceptance criteria

- [ ] `admin_api/client_instances.rs` exists with the listed contents.
- [ ] `server.rs` no longer defines any of the listed items.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.

## Out of scope

- Splitting the `do_admin_client_instance_create` /
  `_update` shared logic into a helper. Pure move.
- Renaming the URL surface (`/v1/admin/client-instances` stays).
