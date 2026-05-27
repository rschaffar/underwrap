# Task: Backend — variant-aware evaluation + admin API

Part of: `epic_single_instance_wrapper_redesign_2026-05`.
Depends on: `1500`, `1510`.

## Problem

The backend is the sole policy authority. Once the runtime wire
carries `variant: WrapperVariant`, the backend must apply the
epic's matrix, and the admin API must round-trip the new
`UnsafeDebugUntil` mode.

## What to do

### 1. `crates/backend-backendd/src/runtime_api/invocations.rs`

`resolve_invocation` gains `variant: WrapperVariant` and applies:

| mode                           | `Real`           | `Debug`                           |
|--------------------------------|------------------|-----------------------------------|
| `Locked`                       | Deny             | Deny                              |
| `Unlocked`                     | Allow            | Deny `"not in unsafe-debug mode"` |
| `UnlockedUntil` not expired    | Allow            | Deny `"not in unsafe-debug mode"` |
| `UnlockedUntil` expired        | Deny `"expired"` | Deny `"expired"`                  |
| `UnsafeDebugUntil` not expired | Allow            | Allow                             |
| `UnsafeDebugUntil` expired     | Deny `"expired"` | Deny `"expired"`                  |

Both the open and the finalize handler read `variant` from the
request and pass it in. The `writable` re-check at finalize is
unchanged.

### 2. `crates/backend-backendd/src/admin_api/policy.rs`

`AdminWrapperPermissionUpsertRequest::mode` is already a
`WrapperApprovalMode`, so the new variant flows through. Confirm no
validation rejects the new tag; add tests.

### 3. Tests

- `runtime_api/invocations/tests.rs`: the full matrix —
  `(Debug, Unlocked) → Deny "not in unsafe-debug mode"`,
  `(Debug, UnsafeDebugUntil future) → Allow`,
  `(Real, UnsafeDebugUntil future) → Allow`,
  expired cases → `Deny "expired"`.
- `admin_api/policy/tests.rs`: upsert + list + delete with
  `mode = UnsafeDebugUntil { until_epoch }`; assert the JSON shape
  is `{ kind: "unsafe-debug-until", until_epoch }`.

### 4. OpenAPI

Confirm `/v1/openapi.json` reflects the new mode + `WrapperVariant`
(both derive `utoipa::ToSchema` in tickets 1500/1510). Ticket 1560
regenerates `web/src/api/schema.ts` from it.

## Acceptance

- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cargo test -p backend-backendd`
- `curl` round-trip of an `unsafe-debug-until` permission via the
  admin API succeeds.
