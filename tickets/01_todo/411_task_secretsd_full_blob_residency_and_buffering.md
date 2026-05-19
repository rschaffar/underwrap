# Task: secretsd full-blob residency and buffered mutations

Parent: `410_story_wrapper_buffered_writeback_and_warnings.md`

## Scope

Rewrite `crates/frontend-secretsd/` so it consumes a `ResourceBlob`
at startup (via one `Checkout` over the channel fd) and serves every
D-Bus call from the in-process `LibsecretSection` of that blob,
mutating it in place. No per-call traffic to underwrapd during the
run.

### Boot path

`main.rs` parses args (`--channel-fd`, `--resource-id` — new arg
replacing the implicit "everything goes through this channel"
model), sends one `WrapperResourceRequest::Checkout { resource_id }`
on fd 4, receives `CheckoutOk { blob, writable }`, decodes the
blob, and hands the in-memory `LibsecretSection` to the D-Bus
service.

PR_SET_DUMPABLE=0 stays the very first action.

### Service / collection / item interfaces

Every mutating D-Bus path that today calls `channel::save_*`:

- `Service.CreateCollection` / `Service.DeleteCollection` /
  `Service.SetAlias`
- `Collection.CreateItem` / `Collection.Delete` / `Collection.SetLabel`
- `Item.SetSecret` / `Item.SetLabel` / `Item.SetAttributes` /
  `Item.Delete`

is rewritten to mutate the in-memory `LibsecretSection` only.

Reads:

- `Item.GetSecret` and `Item.GetSecrets` return secret bytes from
  the in-memory section. (This is the behavior change called out in
  the epic — secret bytes are now resident in secretsd memory for
  the namespace lifetime.)

### Teardown

secretsd exposes the current `LibsecretSection` (or the full blob,
if simpler) to `frontend-ns` at teardown time. The plan in task
413 is for `frontend-ns` to be the single end-of-run decider that
sees both `LibsecretSection` (from secretsd) and the files tree
(from tmpfs), composes the final `ResourceBlob`, and decides
Checkin vs UsageSignal vs warn. Concretely: secretsd serializes
its current `LibsecretSection` (or writes it to a shared
known-fd) on receipt of a teardown signal from `frontend-ns`.

The exact transport between secretsd and frontend-ns for this
handoff is an implementation detail of this task; recommended
approach is a second pipe fd from `frontend-ns` to secretsd,
distinct from the channel-to-daemon fd, on which `frontend-ns`
writes a one-byte teardown signal and reads a JCS-canonical
`LibsecretSection` framed with the existing IPC framing.

### Channel close

After Checkout, secretsd has no further reason to keep fd 4 open
until teardown — but it does need fd 4 to send the teardown
result. Keep fd 4 open in secretsd for the lifetime of the
namespace; do not pass it to the target. (Existing rule from
CLAUDE.md: "Never pass the secretsd channel fd (fd 4) to the
target process" — still applies.)

## Acceptance criteria

- [ ] secretsd issues exactly one Checkout at startup. Zero per-call
      requests to underwrapd during normal D-Bus traffic.
- [ ] All D-Bus mutating paths update the in-memory section without
      I/O.
- [ ] `Item.GetSecret` serves bytes from the in-memory section.
- [ ] secretsd hands the final `LibsecretSection` (or full blob) to
      `frontend-ns` on teardown.
- [ ] fd 4 is closed before exec'ing the target (unchanged invariant
      from CLAUDE.md).
- [ ] PR_SET_DUMPABLE=0 still set before reading from the channel.
- [ ] Unit tests cover every mutating D-Bus path: each leaves the
      `LibsecretSection` in the expected state, no channel traffic
      observed via a `ChannelOps` test double.
- [ ] `cargo test -p underwrap-secretsd` green; clippy clean.

## Out of scope

- The end-of-run decision (Checkin / UsageSignal / drop) — task 413.
- The journal / stderr warning — task 414.
- Tmpfs writability for directories — task 412.
