# Task: Extract small admin subdomains (`users`, `overview`, `secrets`, `policy`, `pairs`)

Parent: `700_story_backend_modularization.md`

## Scope

Move six low-volume admin subdomains into their own modules. Each is
small enough on its own that batching them in a single ticket is
fine; together they take a chunk out of `server.rs` and leave only the
three large subdomains (`resources`, `client_instances`, `stores`)
plus the dispatcher.

### `crates/backend-backendd/src/admin_api/users.rs` (new)

Move verbatim from `server.rs`:

- `user_list_item` (2698-2705)
- `user_detail` (2707-2714)
- `write_admin_user_audit` (2716-2738)
- `mutate_collection` (2740-2750)
- `handle_admin_users_list` (2752-2768)
- `handle_admin_user_detail` (2770-2787)
- `handle_admin_user_create` (2789-2800) + `do_admin_user_create` (2802-2862)
- `handle_admin_user_update` (2864-2876) + `do_admin_user_update` (2878-2920)
- `handle_admin_user_reset_password` (2922-2993)
- `handle_admin_user_delete` (2995-3042)

### `crates/backend-backendd/src/admin_api/overview.rs` (new)

Move verbatim from `server.rs`:

- `admin_ui_overview_for_store` (1436-1477)

Owns the `/v1/admin/ui/overview` GET handler that's currently inlined
in `handle_admin_request` (5904-5939). Extract it into a `pub(crate)
fn handle_admin_ui_overview(state) -> Response` here.

### `crates/backend-backendd/src/admin_api/secrets.rs` (new)

Move verbatim from `server.rs`:

- `handle_admin_secret_write` (1221-1265)
- `handle_admin_secret_rename` (1273-1361)

Plus inlining the `/v1/admin/secret/{read,list,delete}` arms from
`handle_admin_request` (5940-6048) as `pub(crate) fn
handle_admin_secret_{read,list,delete}` so the dispatcher just
forwards.

### `crates/backend-backendd/src/admin_api/policy.rs` (new)

Pull out the `/v1/admin/policy/*` and
`/v1/admin/wrapper-permissions/*` arms (6050-6128, 5493-5640):

- `drop_client_instance_permissions` (4795-4814)
- `write_admin_permission_audit` (4816-4847)
- `handle_admin_wrapper_permissions_list` (5493-5511)
- `handle_admin_wrapper_permission_upsert` (5513-5582)
- `handle_admin_wrapper_permission_delete` (5584-5640)
- `pub(crate) fn handle_admin_policy_get(...)` / `_put(...)` extracted
  from the inline match arms.

### `crates/backend-backendd/src/admin_api/pairs.rs` (new)

Move verbatim:

- `handle_admin_pairs_list` (5276-5303)
- `handle_admin_pair_create` (5305-5392)
- `handle_admin_pair_delete` (5394-5491)

### `crates/backend-backendd/src/admin_api/mod.rs`

Declare the new submodules. `handle_admin_request` updates its match
arms to call the new locations. No routing change.

## Acceptance criteria

- [ ] `admin_api/{users,overview,secrets,policy,pairs}.rs` exist with
      the listed contents.
- [ ] `server.rs` no longer defines any of the listed items, and the
      inline match arms in `handle_admin_request` are reduced to
      one-liners.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.

## Out of scope

- Refactoring `handle_admin_request` into sub-routers (810).
- Splitting `policy.rs` further (the policy document CRUD and
  wrapper-permission CRUD share `drop_client_instance_permissions` /
  `write_admin_permission_audit`, so they stay together).
- Touching `resources.rs`, `client_instances.rs`, `stores.rs` — those
  are 780, 790, 800.
