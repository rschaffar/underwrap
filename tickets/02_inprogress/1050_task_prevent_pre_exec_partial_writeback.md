# Prevent pre-exec partial writeback

Parent: none
Source review: tickets/_review/codebase_2026-07-18.md
Source findings: F-006
Source themes: C-001

## Outcome

After authenticated handoff, target/caller identity resolution, and the required
secret-free idmap helper, a files projection is completely validated before
`prepare_namespace`, idmapped-mount realization, PID-namespace fork, staging,
target-facing helper launch, target-UID switching, or target execution. `underwrap-ns`
authorizes end-of-run dispatch only after the child reports that all controlled
setup is complete and the private CLOEXEC gate then reaches EOF without a
failure marker. A semantic projection error, controlled setup error, normally
reported returned-exec failure, or invalid gate state therefore cannot collect
or submit partial ephemeral staging as the complete durable resource; the exact
post-`Armed` reporting ambiguity is selected and bounded below.

Valid targets retain the current lifecycle: after successful exec, files and
libsecret changes are finalized for exit 0, nonzero exit, and signal death, and
the target's exact exit or signal status is then propagated as today.

## Scope

- Preserve the security-sensitive prerequisite order: authenticate fd 20,
  resolve target/caller identity, create and reap the secret-free idmap helper
  before reading fd 3 or Checkout bytes, then read fd 3 and perform Checkout.
  Complete files planning follows Checkout and precedes `prepare_namespace`,
  idmapped-mount realization, PID-namespace fork, staging tmpfs/materialization,
  bind mounts, target-UID switching, target-facing helper launch, and target
  execution.
- Decode and purely validate the complete files projection at that planning
  point. Validate every configured mount and file key as clean,
  require every key to route strictly to exactly one mount, reject a key equal
  to a mount path, and reject any pair of file keys whose destinations would
  require one path to be both a regular file and a directory, including a file
  and its descendant.
- Produce one completed immutable projection plan and make realization consume
  it. Realization may still report environmental filesystem or mount failures,
  but it must not rediscover semantic route, equality, or file-shape errors after
  writing begins.
- Add a tiny private `underwrap-ns`-local CLOEXEC child-to-parent status gate.
  Create it after the idmap helper is reaped and while fixed fd 3 and fd 20 are
  still occupied, so its descriptors cannot collide with either fixed fd; then
  read fd 3 and proceed with Checkout/planning. Keep the gate out of fd 3, fd 20,
  wire types, `ExecBundle`, configuration, environment, and argv.
- Have the child send exactly one `Armed` marker only after all controlled setup
  and projection materialization is complete, immediately before the target
  exec attempt. Every controlled child-side setup return after the namespace
  fork reports `Failed`. When `CommandExt::exec` returns and the child remains
  alive to follow that return path, it must report `Failed` before exiting;
  failures before a child owns the writer fail closed directly. A completely
  observed `Failed` is never treated as an exec transition.
- Authorize staging collection and finalization only after the parent observes
  exactly `Armed`, then CLOEXEC EOF, with no `Failed`. EOF before `Armed`,
  `Failed`, malformed, truncated, duplicate, trailing, or otherwise extra data,
  gate I/O failure, and child wait/reap failure are fail-closed and authorize no
  collection or finalization.
- Own and close every gate end on every path. The parent and namespace child keep
  only their respective ends, and the target, dbus-daemon,
  `underwrap-secretsd`, and other helpers inherit no gate descriptor. Retain the
  existing rule that the target also inherits neither fd 20 nor another fd above
  stderr.
- Before `Armed`, and on every explicit `Failed`, malformed/truncated/extra gate
  stream, gate I/O failure, or wait/reap failure after Checkout, do not collect
  files or libsecret staging and send neither `Checkin` nor `UsageSignal`.
  Exact `Armed` followed by EOF without `Failed` is the sole authorization
  state, subject only to the irreducible post-`Armed` reporting ambiguity below.
  Leave the backend open operation's existing `last_used_at` update untouched;
  add no abort, rollback, or compensating protocol. Approval of this ticket
  explicitly selects this terminal lifecycle behavior and exception.
- After accepted `Armed` plus CLOEXEC EOF, preserve the existing canonical
  reconstruction and writeback decision for target exit 0, nonzero exit, and
  signal death, followed by exact current exit-code or signal propagation. From
  complete observation of `Armed` until either successful exec closure or
  complete observation of `Failed`, child death is indistinguishable from
  successful CLOEXEC closure. This sole selected exception includes death after
  exec returns but before `Failed` becomes observable. Because all controlled
  setup, validation, and materialization are complete and no target ran, it may
  at most finalize the fully materialized unchanged projection (normally a
  `UsageSignal`), never partial or target-changed state. Do not claim that the
  gate proves exec or provides F-007 signal forwarding/orphan prevention.
- Keep finalization failures diagnostic-only and subordinate to the target
  status. Update current-state lifecycle/security/testing documentation to
  describe complete prevalidation, the private gate, fail-closed pre-exec
  behavior, and the retained post-exec semantics without ticket history or
  stronger atomicity claims.
- Keep the implementation local to the existing privileged stage. There is no
  change to the `underwrap-exec → underwrap-ns` binary chain.

## Non-goals

- Do not absorb F-007–F-011, F-015, F-017–F-019, F-031, F-043, O-011, O-012,
  O-024, or any other finding, unresolved item, or Opportunity.
- Do not add signal forwarding, process-group management, orphan cleanup, or
  another cancellation policy. Do not redesign signal handling merely to make a
  test easier.
- Do not change finalization-error status precedence, supervisor capability
  lifetime, fd-20 authentication or framing, target UID resolution/isolation,
  URL/TLS behavior, backend policy checks, finalize authorization, or the
  documented non-CAS last-writer-wins coordination model.
- Do not change F-001's complete typed `ResourceBlob` ownership, env behavior,
  libsecret behavior or validation boundary, permission modes, writable-flag
  semantics, runtime endpoints, API/wire/config schemas, or public size
  contracts.
- Do not add a dependency, binary, trampoline, ptrace-based exec observer,
  backend abort verb, rollback/CAS mechanism, alternate privileged chain, or
  test-only production fault hook.
- Do not place gate state, secret material, resource blobs, target argv, or
  credentials in fd 3, fd 20, environment variables, argv, wire messages, or
  logs.

## Acceptance criteria

- Complete files planning is pure and follows only authenticated handoff,
  identity resolution, the reaped secret-free idmap helper, fd-3 read, and
  Checkout. It finishes before `prepare_namespace`, idmapped mounts, the
  PID-namespace fork, staging/materialization, bind mounts, target-UID switch,
  target-facing helper launch, or target execution. Every mount and key is clean; every key
  has exactly one strict route; equal mount/key paths and file/descendant
  file-shape collisions are rejected before realization.
- Realization consumes only a completed plan and cannot discover a semantic
  route, equality, or aggregate file-shape error after it starts creating or
  writing staging paths.
- Deterministic red-before unit regressions use the production planning/
  realization path to prove that (a) an earlier valid key followed by a later
  clean but unroutable key and (b) `a` plus `a/b` under one mount both fail with
  zero staging filesystem side effects. Testing only a detached validator is
  insufficient.
- The private gate accepts only exact `Armed` followed by CLOEXEC EOF without
  `Failed`. Focused tests cover EOF-before-armed, explicit failure, malformed,
  truncated, duplicate/extra data, I/O failure, wait failure, and the normal
  returned-exec path that completely reports `Failed`; every completely observed
  failure state is non-finalizable and non-collecting.
- Descriptor-ownership tests cover opposite-end closure, representative
  creation/fork/helper/error paths, helper non-retention, executed-target
  noninheritance of both gate fd and fd 20, and occupied-fd collision safety.
  `OwnedFd`/RAII plus reviewed raw-fd transitions establish closure on remaining
  return paths.
- A production-lifecycle test observes the real parent dispatch decision—not
  only a detached enum or gate parser—and proves a controlled pre-exec failure
  does not invoke staging collection and sends neither `Checkin` nor
  `UsageSignal` even when temporary staging would differ.
- The existing successful-exec path still sends `UsageSignal` for an unchanged
  projection and applies the current writable/read-only `Checkin` behavior for a
  changed projection. Env-only, files-only, libsecret-only, and combined valid
  projections retain their current behavior.
- A deployed NixOS VM case seeds an earlier valid file key and a later clean but
  unroutable key, invokes the real setgid/capability wrapper chain, and proves:
  invocation failure; target non-execution; exact byte equality of the canonical
  backend blob before and after; all seeded keys/values still present; and exact
  post-cursor absence of both `resource_checkin` and
  `resource_usage_signal` in complete `underwrapd` audit records filtered to the
  exact wrapper and stable resource id, not a broad substring grep.
- A separate deployed VM case uses a missing or otherwise unexecutable target so
  `exec` returns after valid setup. It proves the existing failure status and
  diagnostic, exact canonical-blob equality, and the same exact structured,
  post-cursor absence of both terminal audit verbs. Checkout/open and its
  existing `last_used_at`
  bump remain unchanged; the test does not require absence of the open event.
- Deployed post-exec regressions prove that a target which writes a projected
  file and exits 37 persists the exact expected canonical blob and returns 37,
  and that a target which writes then dies by `SIGKILL` persists the exact
  expected canonical blob while a direct Python parent observes
  `-signal.SIGKILL`. These tests add no signal-forwarding or orphan redesign.
- Finalization errors remain diagnostics and do not replace the executed
  target's status. The gate is not documented or tested as solving F-007, and no
  CAS, rollback, or failure-atomic host-filesystem claim is added.
- Backend open/finalize policy re-evaluation, writable recheck, env immutability,
  complete typed blob handling, non-CAS semantics, target UID isolation,
  capabilities, fixed-fd handoff authentication, URL/TLS behavior, and the
  privileged binary chain remain unchanged.
- Published documentation describes only the corrected present-tense
  first-release lifecycle and does not mention F-006, the ticket, migration, or
  superseded alternatives.

## Security boundary

- Protected asset: integrity and availability of the complete durable backend
  resource, including file entries not yet reached when frontend setup fails.
- Attacker: none is required. Root-owned configuration remains trusted; a
  trusted configuration mismatch, aggregate file-shape conflict, filesystem or
  mount fault, helper failure, or missing/unexecutable configured target can
  trigger the boundary.
- Boundary direction: namespace-private ephemeral staging → daemon/backend
  finalization → durable `StorageBackend` resource.
- Invariant: no state other than exact `Armed` followed by CLOEXEC EOF without
  `Failed` collects or submits staging. Child death after `Armed` and before
  either successful exec closure or complete `Failed` observation is
  indistinguishable from CLOEXEC closure; that accepted state may at most
  finalize the already validated, fully materialized unchanged projection and
  can never submit ephemeral partial state as the authoritative complete
  resource.

## Validation

- `cargo test -p underwrap-ns complete_projection_ -- --nocapture`
- `cargo test -p underwrap-ns exec_gate_ -- --nocapture`
- `cargo test -p underwrap-ns pre_exec_failure_skips_collection_and_terminal_verbs -- --nocapture`
- `cargo test -p underwrap-ns`
- `cargo test -p integration-tests`
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `nix build .#checks.x86_64-linux.vm-test`
- Focused VM assertions for exact canonical-blob equality and exact filtered
  `resource_checkin`/`resource_usage_signal` absence on the later-invalid-key and
  returned-exec-error cases, plus exact persistence/status evidence for exit 37
  and `SIGKILL` after successful exec.
- If the implementation introduces a Rust source file, make that file visible to
  the Git-filtered Cargo/Crane source before running the Nix check, following the
  ticket workflow's exact staging or intent-to-add rule. Ordinary Cargo Rust
  sources require no new `flake.nix` source-filter predicate.
- `git diff --check`

No web checks are required because this task changes no SPA or browser surface.

## Dependencies

- None. F-001's completed typed whole-resource correction is a preserved
  invariant, not new work or an implementation dependency.
