# Task: Extract the runtime API (`/v1/wrappers`, `/v1/wrapper-invocations`, …)

Parent: `700_story_backend_modularization.md`

## Scope

Move the three mTLS runtime endpoints and their supporting machinery
into a dedicated `runtime_api/` module. This is the smallest natural
boundary in `server.rs`: only three endpoints, all hit by the daemon,
all already use the helpers extracted in 720 and 740.

### `crates/backend-backendd/src/runtime_api/mod.rs` (new)

Declares `principal`, `wrappers`, `invocations` submodules. Owns
`handle_service_request` (917-987) — the runtime-side dispatcher.

### `crates/backend-backendd/src/runtime_api/principal.rs` (new)

Move verbatim from `server.rs`:

- `RuntimeRequestPrincipal` struct (784-787)
- `resolve_runtime_client` (789-823)
- `validate_runtime_client_mappings` (572-593)
- `get_policy_document` (825-835) and `put_policy_document` (837-848)
  if still referenced; otherwise delete them in this ticket (they
  look unused, but confirm by grep before deleting).

### `crates/backend-backendd/src/runtime_api/wrappers.rs` (new)

Move verbatim from `server.rs`:

- `handle_runtime_wrapper_register` (1490-1736)
- `generate_resource_id` (2088-2096) — used only by this handler
  today; keep it co-located here unless 770/780 also need it.

### `crates/backend-backendd/src/runtime_api/invocations.rs` (new)

Move verbatim from `server.rs`:

- `InvocationLookup` enum (1738-1750)
- `resolve_invocation` (1752-1795)
- `handle_runtime_wrapper_invocation_open` (1797-1910)
- `handle_runtime_wrapper_invocation_finalize` (1912-2086)
- `bump_pair_last_used` (1007-1026)

### Call-site updates

`server.rs` keeps `handle_https_request` for now (it dispatches
admin vs service); the service branch now calls
`crate::runtime_api::handle_service_request`.

## Acceptance criteria

- [ ] `runtime_api/{mod,principal,wrappers,invocations}.rs` exist with
      the listed contents.
- [ ] `server.rs` no longer defines any of the listed functions or
      types.
- [ ] If `get_policy_document` / `put_policy_document` are confirmed
      unused, they are deleted (with a note in the PR description);
      otherwise they move with the rest.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] The integration tests under `integration-tests/` that exercise
      `POST /v1/wrappers`, `/v1/wrapper-invocations`, and
      `/v1/wrapper-invocations/finalize` still pass.

## Out of scope

- Adding or removing endpoints. The set of three is fixed by
  `epic_runtime_api_redesign`.
- Changing wire types or response codes.
- Touching the daemon-side `BackendClient`.
