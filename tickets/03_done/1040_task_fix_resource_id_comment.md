# Task: Correct the `resource_id` "diagnostic only" comments

## Problem

Several comments describe the backend-supplied `resource_id` as purely
diagnostic / never used beyond logging, but it is in fact relayed to the
wrapper and used in audit events and user-facing error messages.

- `RuntimeWrapperInvocationOpenResponse::Allow.resource_id`
  (`crates/shared-protocol/src/service.rs:693-698`) says the daemon
  "relays this to the wrapper inside `WrapperResponse::ReadOk` as an
  opaque identifier; the daemon itself never has to look it up." — the
  first half is right; it is not merely opaque-and-ignored downstream.
- `OpenedInvocation.resource_id`
  (`crates/frontend-underwrapd/src/wrapper_flow/read.rs:52-54`):
  "threaded through to the channel purely for diagnostic logging."
- `ChannelContext.resource_id`
  (`crates/frontend-underwrapd/src/wrapper_resource_channel.rs:46-47`):
  "threaded through for diagnostic logging only."

In reality the wrapper chain uses it for operator-facing output:
`frontend-ns/src/main.rs:399-431` emits `resource_id` in the
`writeback_rejected` structured audit event, in the systemd journal
line, and in the stderr message shown to the user (e.g.
`changes to resource '<name>' (<resource_id>) ...`), and
`frontend-ns/src/writeback.rs` carries it through the writeback outcome.

## What to do

1. Update the three comments above so they reflect actual use: the
   `resource_id` is an opaque backend identifier (the daemon never
   addresses storage by it) **but** it is surfaced to the wrapper and
   downstream into audit events and user-facing writeback messages — it
   is not "diagnostic only."
2. Comment-only change. No code, no wire change.

## Acceptance

- `cargo fmt -- --check`
- Comments accurately describe how `resource_id` is consumed
  (cross-check against `frontend-ns/src/main.rs` and `writeback.rs`).
