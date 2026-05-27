# Task: Shared-protocol — UnsafeDebugUntil policy + matrix

Part of: `epic_single_instance_wrapper_redesign_2026-05`.
Depends on: `1500_task_protocol_instance_config_and_ipc` (provides
`WrapperVariant`).

## Problem

The debug tool is now a runtime `variant`, gated by a new policy
mode. The shared evaluator and the runtime wire need to know about
both.

## What to do

### 1. `crates/shared-protocol/src/policy.rs` — new mode

Add a variant to `WrapperApprovalMode` (alongside `Locked`,
`Unlocked`, `UnlockedUntil`):

```rust
/// Allow invocations of both the real wrapper and its debug
/// variant until `until_epoch`; deny afterwards. Checked lazily.
UnsafeDebugUntil { until_epoch: u64 },
```

The existing serde config (`tag = "kind"`, kebab-case) yields the
wire name `"unsafe-debug-until"` automatically.

### 2. `decide` — variant-aware matrix

Extend `decide` (and `PolicyDocument::evaluate_permission{_at}`)
with `variant: WrapperVariant`:

| mode                           | `Real`           | `Debug`                           |
|--------------------------------|------------------|-----------------------------------|
| `Locked`                       | Deny             | Deny                              |
| `Unlocked`                     | Allow            | Deny `"not in unsafe-debug mode"` |
| `UnlockedUntil` not expired    | Allow            | Deny `"not in unsafe-debug mode"` |
| `UnlockedUntil` expired        | Deny `"expired"` | Deny `"expired"`                  |
| `UnsafeDebugUntil` not expired | Allow            | Allow                             |
| `UnsafeDebugUntil` expired     | Deny `"expired"` | Deny `"expired"`                  |

The "not in unsafe-debug mode" reason should name the wrapper +
resource, matching the existing message style.

### 3. Runtime wire — `crates/shared-protocol/src/service.rs`

Add `variant` to both runtime requests:

```rust
// RuntimeWrapperInvocationOpenRequest and
// RuntimeWrapperInvocationFinalizeRequest:
#[serde(default)]
pub variant: WrapperVariant,
```

Both derive paths through `utoipa::ToSchema` so the OpenAPI doc and
the SPA's generated schema pick up the new mode + the
`WrapperVariant` enum.

### 4. Tests — `crates/shared-protocol/src/policy/tests.rs`

Sibling-file submodule pattern. Add:

- every (variant × mode) cell from the matrix;
- the `UnsafeDebugUntil` "not in unsafe-debug" deny reason for
  `Debug` under `Unlocked` / `UnlockedUntil`;
- JSON round-trip of `UnsafeDebugUntil { until_epoch }` locking in
  the kebab-case tag.

## Out of scope

- Backend wiring of the matrix (ticket 1540).
- Daemon variant relay (ticket 1530).

## Acceptance

- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cargo test -p shared-protocol`
