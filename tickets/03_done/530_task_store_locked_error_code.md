# Task: `ErrorCode::StoreLocked` + HTTP mapping

Parent: `500_story_in_memory_master_keys.md`
Epic: `epic_in_memory_master_keys.md`

## Scope

- In `crates/shared-protocol/src/message/errors.rs`:
  - Add `StoreLocked` to `ErrorCode` with wire discriminant `4`.
  - Update the `WireEncode` / `WireDecode` impls, including the
    "invalid discriminant" branch and unit tests for round-trip.
  - Bump any wire-version constant if one exists (search for
    `WIRE_VERSION` / `PROTOCOL_VERSION`).
- In the service HTTP layer of `backend-backendd`:
  - Every handler that calls `registry.unlocked_backend(...)` maps
    `RegistryLookupError::Locked → StatusCode::SERVICE_UNAVAILABLE`
    with body `{"code":"store_locked", "store_id": "..."}`.
  - `NotFound` keeps its existing mapping.
  - Touch points (non-exhaustive — grep
    `state.stores.read()` after 520 lands):
    `handle_service_read_prefixes`,
    `handle_service_write_prefixes`,
    `handle_service_list`, `handle_service_delete`,
    `handle_service_checkout`, `handle_service_checkin`,
    `handle_service_usage_signal`, plus admin secret-reveal /
    secret-list endpoints.
- Frontend daemon (`crates/frontend-underwrapd`): propagate the
  `StoreLocked` error from the service client to the wrapper IPC
  layer. The wrapper-side `WrapperResponse` already has an
  `Error { code, message }` shape (or equivalent); add a
  `StoreLocked` variant there too **only if** the existing shape
  cannot carry the new `ErrorCode` faithfully. Prefer carrying the
  existing `ErrorCode` through.
- `frontend-exec`: when the wrapper receives `StoreLocked`, exit
  non-zero and print a single stderr line
  `underwrap: store '<name>' is locked; ask an administrator to unlock
  it`. No retries, no fallback.
- Tests:
  - `shared-protocol`: round-trip + invalid-discriminant unit tests.
  - `backend-backendd`: a `read_prefixes` request against a locked
    encrypted store returns 503 + `store_locked`.
  - `integration-tests`: spin up the backend with a locked encrypted
    store and verify the frontend daemon translates the error all the
    way to the wrapper exit code + stderr line.

## Acceptance criteria

- [ ] `ErrorCode::StoreLocked` round-trips on the wire.
- [ ] Every service handler that needs a backend uses
      `unlocked_backend` and surfaces 503 + `store_locked` when locked.
- [ ] Wrapper user-facing error is one clear line.
- [ ] `cargo test --workspace` green.

## Out of scope

- Admin endpoints `unlock` / `lock` (540).
- Canary file (created in 550, consumed by 540).
