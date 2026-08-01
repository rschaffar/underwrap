# Prevent supervisor termination from orphaning targets

Parent: none
Source review: tickets/_review/codebase_2026-07-18.md
Source findings: F-007
Source themes: C-003

## Outcome

Termination of either trusted wrapper supervisor closes the invocation instead
of leaving a detached, distinct-UID, secret-bearing target namespace running.
`underwrap-exec` and the host-side `underwrap-ns` supervisor are connected to the
namespace by race-closed parent-lifetime links. A trusted, non-execing
`underwrap-ns` relay remains namespace PID 1, and the configured target runs as
namespace PID 2 in its own session. Killing PID 1 therefore lets the kernel
terminate every target, helper, and descendant even when the configured target
executes a set-ID or file-capability image, changes process groups, or forks.

When all supervisors remain healthy, direct target exit 0, nonzero exit, and
signal death retain the F-006 lifecycle: clean relay reap proves complete
in-namespace process/PID-namespace teardown before files/libsecret collection
and the existing `UsageSignal`/`Checkin` decision, and the target's exact exit
code or signal is then propagated. The host supervisor intentionally retains the
private mount namespace only for bounded collection and releases it when the
invocation ends. Actual supervisor or relay death while the target namespace is
still live is fail-closed cancellation and tears that namespace down. A
cancellation or relay failure established before resource-terminal dispatch
permits no staging collection, `Checkin`, or `UsageSignal`. A terminal request
that completed before a later stage-1 death is not retracted; without a backend
abort/transaction there is an unavoidable race from terminal dispatch until the
outer wrapper exits. No rollback, new writeback, or status can be produced by a
supervisor that no longer exists.

Approval of this ticket explicitly selects an observable first-release topology
change: the trusted relay is namespace PID 1 and the configured target is PID 2
with PPID 1. It also approves one new fixed private liveness fd on the existing
`underwrap-exec → underwrap-ns` handoff, one always-present relay process inside
`underwrap-ns`, unconditional `SIGKILL` cascading on parent death, and the abrupt
cancellation semantics above. The two existing binaries and configured target
selection remain unchanged; no new executable is introduced.

## Scope

- Preserve fd 20 as the first authority decision. After `underwrap-ns` uses only
  the instance selector needed to authenticate fixed fd 20, but before it trusts
  the wrapper selector, target argv, fd 3, target identity, idmap input,
  Checkout, or privileged namespace work, validate and adopt one new fixed
  parent-liveness fd supplied by `underwrap-exec`.
- Create the outer liveness channel in `underwrap-exec` with
  `pipe2(O_CLOEXEC)`. Its waiting parent owns the sole writer. The child closes
  that writer, maps the reader to a compile-time fixed local-ABI fd that cannot
  collide with stdio, fd 3, or fd 20, and carries only the reader across the
  file-capability exec. `underwrap-ns` validates the expected read-only pipe
  endpoint and required descriptor/status flags, restores CLOEXEC, installs
  `PR_SET_PDEATHSIG(SIGKILL)`, and then checks for EOF/HUP/error. Parent death
  before arming is detected by the pipe; parent death after arming delivers the
  uncatchable signal. Malformed descriptors, ownership/flag errors, and syscall
  failures abort before secret-bearing input.
- Keep the outer writer exclusively in the waiting `underwrap-exec` parent until
  the child has been reaped. Neither `underwrap-ns`, the relay, target, helpers,
  nor descendants retain a writer; no process retains the outer reader after
  the arm-and-check role is complete.
- Before the PID-namespace fork, create a second private CLOEXEC liveness pipe.
  The host `underwrap-ns` supervisor owns the sole writer and namespace PID 1
  owns the reader; each branch immediately closes the opposite inherited end.
  PID 1 arms `PR_SET_PDEATHSIG(SIGKILL)` and checks liveness immediately after
  fork. Because the dedicated UID/GID transition clears the setting, it repeats
  the arm-and-check immediately after the complete credential switch and before
  staging, projection materialization, target-facing helpers, or target fork.
  It then closes the liveness reader. PID 1 never execs, so arbitrary
  configured-target exec behavior cannot clear this final parent-death link.
- Make trusted `underwrap-ns` code remain namespace PID 1 as a minimal relay. It
  performs the existing controlled namespace, mount, projection, target-identity,
  `no_new_privs`, capability-drop, keyring, core-limit, dumpability, and
  credential-verification setup. Before it supervises the target it is the
  dedicated target UID/GID, non-dumpable, `NoNewPrivs=1`, and has zero effective,
  permitted, inheritable, and ambient capabilities. It closes fd 20 with a
  checked operation before the target fork, retains no resource-channel
  capability in either relay or target branch, and gains no new authority.
- Preserve the target-to-caller session boundary. The relay severs its own
  controlling-terminal relationship before the target fork. The PID-2 target
  branch starts a separate session before any target-facing helper fork and
  target exec, so the configured target remains a session/process-group leader,
  has no controlling terminal, and retains only inherited stdio. Keep the fresh
  PID/mount/IPC/UTS namespaces, remounted `/proc`, dedicated UID, cleared
  environment, allowlisted target environment, anonymous keyring, core limit,
  `no_new_privs`, zero capabilities, and final fd sweep.
- Transfer the F-006 gate writer to the PID-2 target-preexec branch only. The
  relay closes its copy after fork. The target branch retains the exact protocol:
  it sends `Armed` only after all controlled setup and final descriptor
  protection, a controlled return sends `Failed`, and successful target exec
  closes the CLOEXEC writer. Libsecret helpers inherit no gate descriptor.
- Add a private bounded relay-to-host status pipe. The host supervisor owns the
  sole reader and PID 1 owns the sole writer. Use one exact fixed/versioned frame
  for the direct PID-2 target/preexec child's terminal wait result:
  `Exited(0..=255)` or `Signaled(signal)` produced only by `WIFSIGNALED` with
  `1 <= signal < NSIG`, followed by EOF. PID 2 closes its inherited status
  writer as its first branch action, before signal-state restoration, helpers,
  or descendants; helpers inherit neither end and cannot forge a status.
- Have PID 1 wait for the direct PID-2 child, write its one terminal-status frame
  atomically in no more than `PIPE_BUF`, close the writer, and exit 0. PID-1 exit
  triggers kernel teardown of every remaining namespace process, including
  private D-Bus helpers and detached descendants. The host first observes the
  F-006 gate and reaps the relay. Only after a clean relay reap does it parse the
  status reader nonblockingly for one exact frame plus EOF; `EAGAIN` after the
  frame proves an out-of-namespace writer remains and fails without waiting.
  Descriptor tests, rather than post-teardown parsing, prove that no
  in-namespace process ever retained the writer.
- Require three independent facts before the real collection and resource
  terminal-dispatch path: exact F-006 gate acceptance, exactly one valid status
  frame, and
  successful exit/reap of the relay. Empty, truncated, malformed, out-of-range,
  duplicate, trailing, retained-writer, I/O, relay-exit, relay-signal, or wait
  failures authorize no collection, `Checkin`, or `UsageSignal`. A valid frame
  plus clean relay remains eligible for exact process-status propagation when a
  `Failed` or otherwise invalid F-006 gate suppresses resource finalization;
  controlled pre-exec failure therefore retains its exact PID-2 failure status.
  Missing/invalid status or an unclean relay produces a generic supervisor
  failure and no terminal resource verb.
- Preserve ordinary completion ordering. After the gate/status/relay conjunction
  and complete PID-namespace process teardown, reconstruct the same typed resource and run
  the existing writable/read-only/backend-policy decision. Unchanged state sends
  one `UsageSignal`; changed writable files/libsecret state sends one `Checkin`;
  changed read-only state retains `UsageSignal` plus `DroppedReadOnly` behavior.
  Finalization errors remain diagnostic-only and subordinate to the framed
  direct-target status.
- Treat actual death of `underwrap-exec`, the host `underwrap-ns` supervisor, or
  the PID-1 relay while the target namespace is live as cancellation. The inward
  cascade uses `SIGKILL`, cannot be caught or blocked, and eventually destroys
  the complete PID namespace. If cancellation/relay failure is established
  before resource-terminal dispatch, collect nothing and send neither `Checkin`
  nor `UsageSignal`. Do not retract a request already completed before a later
  stage-1 death, and document the unavoidable dispatch-to-wrapper-exit race.
  Retain the already completed backend-open `last_used_at` update and add no
  compensating protocol.
- Normalize only signal state required for supervision. Capture inherited
  target-visible `SIGCHLD` disposition, flags, and mask before normalization.
  Each waiter establishes default, non-`SA_NOCLDWAIT`, unblocked `SIGCHLD` before
  its relevant fork so no child can be auto-reaped. Child branches restore the
  captured state before the next stage's exec where required, and the PID-2
  target branch restores it before any target-facing helper fork or target exec;
  supervisors retain the reliable wait state. Exact process-status propagation
  resets and unblocks the direct target's terminating signal before
  self-delivery. Do not add graceful forwarding or otherwise alter target signal
  policy. Representative SIGINT/SIGTERM cancellation tests exercise normal
  supervisor dispositions; SIGKILL is the uncatchable invariant proof.
- Own all new descriptors with RAII except reviewed dup/fork/exec transitions.
  Explicitly close opposite ends on every branch and close liveness/status/gate,
  fd 3, fd 20, and libsecret setup descriptors before helpers or target can
  retain them. Keep `close_range(..., CLOEXEC)` as the final backstop rather than
  a substitute for correct earlier ownership.
- Update present-tense architecture, security, Secret Service security, testing,
  and affected Nix/VM comments. State the relay/target topology, two death links,
  finalization conjunction, normal status/writeback behavior, no terminal verb
  after cancellation is established pre-dispatch, the unavoidable
  dispatch-to-outer-exit race, observable PID/PPID change, eventual teardown
  limit, and denial-of-service limits. Do not claim that sharing the dedicated target
  UID alone lets the configured target signal namespace PID 1 to death: Linux's
  namespace-init signal restrictions still apply. Make no exec-proof,
  transactional cancellation, adversarial-sandbox, or graceful-forwarding
  claim.

## Non-goals

- Do not absorb F-008–F-012, F-015, F-017–F-019, F-031, F-039, F-043, F-045,
  F-046, O-009, O-011, O-012, O-021, O-024, or any other finding, unresolved
  item, or Opportunity. F-006 is a completed prerequisite and regression
  surface, not new scope.
- Do not add graceful signal forwarding, handlers for target cancellation,
  grace periods, escalation policy, timeouts, process-group fanout, a target
  cooperation requirement, or a promise that the target receives the same
  cancellation signal. Do not treat `SIGSTOP` as termination.
- Do not add cancellation-time writeback, rollback, a backend abort/cancel verb,
  compensating `UsageSignal`, transactional semantics, or a machine-status
  guarantee after a supervisor has died.
- Do not add cgroups/systemd transient scopes, pidfd orchestration, `CAP_KILL`, a
  capability increase, another daemon/service/binary/package, a trampoline, or
  a dependency. Do not restrict configured targets from set-ID/file-capability
  exec as a substitute for the relay.
- Do not change fd-20 authentication, target UID authority, resource-channel
  framing, `ExecBundle`, backend/runtime APIs, configuration/Nix options,
  storage formats, permission modes, writable checks, env immutability,
  last-writer-wins semantics, URL/TLS behavior, or the two-binary configured
  `underwrap-exec → underwrap-ns` chain.
- Do not fix target replacement of libsecret state (F-008), resource-size
  mismatch (F-009), Secret Service transaction/validation findings, channel
  deadlines/stalls, finalization-error precedence (O-011), or supervisor
  capability-lifetime reduction (O-012) while touching adjacent code.
- Do not claim that sharing the dedicated target UID alone lets the configured
  target signal namespace PID 1 to death; Linux's namespace-init signal rules
  restrict signals from PID-namespace members. Target-triggered resource
  exhaustion and other denial of service remain outside the threat model. A
  relay actually terminated from the ancestor namespace or by an internal fault
  still fails closed, and the target cannot inherit or forge the relay-only
  status writer.
- Do not redesign F-006's exact gate or its narrow post-`Armed`,
  pre-observable-exec ambiguity. Extend the finalization predicate without
  claiming that either private channel proves exec.

## Acceptance criteria

- Static and production-path inspection prove the exact topology and ownership:
  `underwrap-exec` waiting parent → host `underwrap-ns` supervisor → trusted
  namespace PID-1 relay → configured target PID 2. The target reports PPID 1 and
  remains its own session/process-group leader without a controlling terminal.
- The outer fixed liveness fd is admitted only after fd-20 peer authentication
  and before wrapper selector, target argv, fd 3, identity/idmap, Checkout, or
  namespace work. It is an invocation-local liveness proof admitted only after
  fd-20 authentication, never authority, serialized metadata, target
  environment, argv, or wire state. Anonymous creation and sole-writer ownership
  are construction/branch invariants, not properties inferred from the reader.
- Both parent-death links use production `PR_SET_PDEATHSIG(SIGKILL)` arm/check
  helpers plus sole-writer liveness pipes. Deterministic bounded tests cover
  parent death before arm, after arm, and across the inner credential-clear/rearm
  window; every test reaps or kills all survivors. The deployed file-capability
  `underwrap-ns` boundary is rearmed, and a configured file-capability target
  cannot escape relay teardown or gain capabilities under `no_new_privs`.
- The relay is dedicated-target UID/GID, non-dumpable, `NoNewPrivs=1`, and zero
  capability after setup. The configured target and helpers retain the existing
  identity, capability, namespace, keyring, core, environment, mount, D-Bus,
  stdio, and fd boundaries. The deployed `underwrap-ns` wrapper capability set
  is byte-for-byte semantically unchanged and contains no `CAP_KILL`.
- After a clean relay reap, the private nonblocking target-status parser accepts
  only one complete valid frame followed by EOF. Focused tests reject empty,
  truncated, malformed, out-of-range, duplicate, trailing/extra, I/O-failure,
  and retained out-of-namespace writer streams without hanging; descriptor tests
  separately prove in-namespace non-retention.
- A production lifecycle matrix—not detached parser tests—covers the F-006 gate ×
  target status × relay-reap conjunction. Every invalid gate/status/relay/wait
  cell uses changed temporary staging where applicable, invokes no collection,
  and writes zero `Checkin`/`UsageSignal` request bytes on fd 20. A valid
  pre-exec child status still propagates when the gate alone suppresses resource
  finalization. Relay death after `Armed` cannot be mistaken for ordinary target
  signal death.
- Descriptor tests cover sole-writer/opposite-end closure, fixed-fd collision
  safety, fork and error paths, target/helper non-retention, and successful
  target exec. Neither target nor helpers inherit either liveness channel, the
  status channel, gate, fd 3, fd 20, or another internal descriptor above
  stderr.
- Waiter tests cover inherited ignored/no-child-wait and blocked `SIGCHLD` state,
  reliable supervisor reaping, and restoration of the captured target/helper-
  visible state before helper fork/target exec. Exact status tests prove that a
  blocked terminating signal is reset/unblocked by each propagation stage and
  reaches a direct observer as that signal rather than a normalized exit code.
- Deterministic red-before deployed evidence kills the current `underwrap-exec`
  and host `underwrap-ns` separately and proves the exact recorded target or
  descendant namespace survives on the pre-fix tree; bounded cleanup removes
  every survivor before the test exits.
- A deployed combined files+libsecret long-running target creates a descendant
  and keeps dbus-daemon/`underwrap-secretsd` live. Separate cases kill
  `underwrap-exec`, host `underwrap-ns`, and PID-1 relay with `SIGKILL`; within a
  bounded deadline every recorded supervisor, relay, target, descendant, helper,
  PID namespace, and mount namespace disappears. Representative SIGINT and
  SIGTERM cases against both outer supervisor stages prove the same teardown and
  exact observer signal wherever an observer survives.
- VM process assertions bind every recorded PID to start time and namespace
  identity rather than name/PID alone. After teardown no process retains the
  invocation's PID-namespace or mount-namespace inode. Audit absence starts from
  an exact journal cursor and parses complete records filtered to the stable
  resource id, wrapper, and exact `resource_checkin` and
  `resource_usage_signal` verbs. Every negative case first proves that the
  long-running target/relay has not completed and synchronizes cancellation or
  relay failure before resource-terminal dispatch; only those pre-dispatch cases
  require absence of both terminal verbs. A separately documented late
  stage-1-death case may retain a terminal request that completed first.
- Healthy deployed regressions cover unchanged success, changed exit 37, direct
  target `SIGTERM`, and direct target `SIGKILL`. They prove exact
  `UsageSignal`/`Checkin`, exact files and representative libsecret persistence,
  complete namespace teardown, and direct-parent exit/signal status. Existing
  F-006 exit-37/SIGKILL writeback evidence remains.
- VM posture evidence proves relay namespace PID 1; target PID 2/PPID 1;
  target PID/process-group/session identity and `tty_nr=0`; relay/target
  dedicated UID, `NoNewPrivs=1`, and zero capability; fresh PID/mount/IPC/UTS
  namespaces; remounted `/proc`; and absence of every private internal fd.
- Host/ancestor-namespace termination of a confirmed-live relay before target
  completion is covered as fail-closed: the complete PID namespace disappears,
  the status cannot be forged, and no terminal resource verb is sent. Tests and
  docs make no same-UID target-to-PID-1
  signal claim; target-triggered resource exhaustion remains a denial-of-service
  non-goal.
- Ordinary env-only, files-only, libsecret-only, and combined projections retain
  typed resource ownership, permission/writable behavior, finalization-error
  precedence, and exact exit/nonzero/signal propagation. F-006's invalid-gate,
  pre-exec, exit-37, and target-SIGKILL regressions remain green.
- Published documentation describes only the corrected present-tense
  first-release lifecycle and its limits. It does not mention F-007, the ticket,
  migration, or superseded alternatives.

## Security boundary

- Protected asset: bounded lifetime of resource values in target/helper/
  descendant memory and environment, namespace-private files/tmpfs and private
  D-Bus/libsecret state, plus the invocation descriptors that can reach them.
- Attacker: the untrusted local caller or its shell/service manager may cancel
  the invocation or kill its same-UID `underwrap-exec`; either trusted supervisor
  may also crash or be terminated. Root and kernel compromise remain out of
  scope. The configured target remains trusted with its projection; target-
  triggered resource exhaustion and other denial of service remain non-goals.
- Boundary direction: caller/service-manager termination → `underwrap-exec` →
  host `underwrap-ns` supervisor → namespace PID-1 relay → complete
  secret-bearing namespace.
- Invariant: after actual termination of either outer trusted supervisor, the
  relay, configured target, helpers, descendants, and their PID namespace
  eventually disappear, subject to kernel scheduling/uninterruptible-sleep
  limits; a surviving host supervisor retains the private mount namespace only
  through bounded fail-closed cleanup. When all supervisors are healthy, staging
  collection and a terminal resource verb require exact F-006 gate acceptance,
  one valid direct-child status, and a cleanly reaped relay; every missing or
  invalid proof fails closed. Root-held external namespace references are out of
  scope.

## Validation

- `cargo test -p underwrap-exec`
- `cargo test -p underwrap-ns liveness_ -- --nocapture`
- `cargo test -p underwrap-ns target_status_ -- --nocapture`
- `cargo test -p underwrap-ns relay_ -- --nocapture`
- `cargo test -p underwrap-ns lifecycle_conjunction_ -- --nocapture`
- `cargo test -p underwrap-ns`
- `cargo test -p integration-tests`
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `nix build .#checks.x86_64-linux.vm-test`
- Focused VM evidence for pre-fix orphan reproduction; stage-1/stage-2/relay
  SIGKILL teardown; representative SIGINT/SIGTERM teardown; exact process and
  namespace identities; no terminal audit verbs for synchronized pre-dispatch
  cancellation; the late-dispatch race; healthy
  success/exit-37/target-SIGTERM/target-SIGKILL writeback and status; unchanged
  capability posture; configured file-capability target containment; and no
  private-fd inheritance.
- If implementation adds Rust source files, expose only those approved files to
  the Git-filtered Cargo/Crane source before the Nix check using exact staging or
  intent-to-add. Ordinary Rust source needs no new `flake.nix` filter predicate.
- `git diff --check`

No web or browser check is required because this task changes no SPA surface.

## Dependencies

- Completed prerequisite:
  `tickets/03_done/1050_task_prevent_pre_exec_partial_writeback.md` (F-006;
  final fix `5b394e9c22754afb39bcb08c8eff8df77ba4995f`).
- No unresolved implementation dependency. F-008 and the remaining C-003
  findings are adjacent and remain separately gated.
