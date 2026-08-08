# Replace target-writable libsecret state with validated publication

Parent: none
Source review: tickets/_review/codebase_2026-07-18.md
Source findings: F-008
Source themes: C-003

## Outcome

A libsecret-enabled invocation no longer accepts final state through a pathname
owned by the configured target UID. `underwrap-ns` creates one invocation-private
bidirectional Unix stream socket before the PID-namespace fork. The host
`underwrap-ns` supervisor owns the sole receiver/validator endpoint and
`underwrap-secretsd` owns the sole long-lived publisher endpoint. The configured
target, descendants, namespace PID-1 relay, dbus-daemon, and other helpers retain
no publication endpoint once their setup role ends.

secretsd publishes generation zero before readiness and publishes a new complete
canonical generation for each D-Bus mutation. It waits for an exact
generation-matched host `ACK` or `REJECT` before readiness or mutation success.
The host bounds, parses, canonical-byte-checks, semantically validates, and
retains a generation before completely writing `ACK`. The target-visible tmpfs
continues to hold the private D-Bus socket and helper logs, but no final-state
pathname is an input to host writeback.

The host selects only the latest fully validated generation whose exact ACK was
completely written. A later rejected, partial, poisoned, response-failed, or
publisher-death attempt preserves that earlier generation after terminal EOF;
it never substitutes partial bytes or reparses Checkout/pathname bytes. A final
nonblocking `EAGAIN` after clean relay reap means a publisher endpoint remains
open and fails closed immediately: no collection, `Checkin`, or `UsageSignal`,
and no latest-generation fallback. Exact target exit/signal status can still
propagate when its existing gate/status/reap evidence is valid.

Approval of this ticket explicitly selects these observable first-release
semantics:

- generation zero must receive complete host `ACK(0)` before secretsd readiness,
  `Armed`, or target execution;
- D-Bus success means the trusted host retained the generation and completely
  wrote its ACK, not that the caller received the response or the backend
  committed it;
- a host ACK followed by secretsd death before the caller receives success is an
  acknowledged response-loss window, and the ACKed generation remains eligible;
- the latest earlier ACK-written generation survives a later ordinary rejected,
  partial, transport-failed, or publisher-death attempt when terminal EOF and
  the complete F-006/F-007 lifecycle conjunction are observed;
- a still-open publication endpoint is different: it authorizes no collection
  or terminal resource verb and cannot use fallback;
- strict validation can reject malformed pre-release Checkout state before
  target execution and can turn a malformed mutation into D-Bus `Failed`;
- libsecret-only collection no longer enters the target-fsuid window; files
  collection retains its existing bounded target-fsuid behavior.

No exactly-once D-Bus response or backend-durability claim is added. The generic
resource channel's separate 64 KiB limit remains F-009, so a private publication
ACK near 1 MiB is not a promise that the complete resource can yet be checked in.

## Scope

- Reproduce F-008 before implementation through the real pre-fix path. A live
  same-UID target creates/replaces/renames the predictable
  `libsecret-state.jcs` entry with attacker-selected canonical, noncanonical but
  parseable, over-1-MiB-within-tmpfs, and semantically invalid state. Record
  bounded red evidence that the current host pathname consumer can select an
  attacker-controlled parseable generation for composition/writeback. Clean up
  every process and never place secret payloads in argv or logs.
- After Checkout confirms libsecret is enabled and before the PID-namespace
  fork, create one private `AF_UNIX`, `SOCK_STREAM | SOCK_CLOEXEC` socketpair.
  Keep it out of fd 3, fd 20, fixed fd 21, the F-006 gate, target-status channel,
  `ExecBundle`, environment, public configuration, and backend/daemon protocols.
  Continue using the existing one-shot init pipe for Checkout bytes.
- Give the host branch only the receiver endpoint and close its publisher copy.
  Give the namespace branch only the publisher endpoint. PID 1 closes its copy
  immediately after forking PID 2 and continues its existing active liveness and
  direct-child supervision without parsing or retaining libsecret generations.
- PID 2 keeps the publisher only while launching secretsd. dbus-daemon explicitly
  closes it. The secretsd exec child alone clears CLOEXEC on the exact descriptor
  for that exec; secretsd immediately restores CLOEXEC and adopts the descriptor
  through RAII after setting itself non-dumpable. PID-2 setup closes its copy
  immediately after the secretsd fork and before readiness, `Armed`, final fd
  protection, helpers exposed to the target, or target exec.
- Ensure secretsd inherits no fd 20, gate, liveness, target-status, or
  D-Bus-address descriptor. It inherits only its publication endpoint and the
  exact one-shot init/readiness descriptors required for startup, closes each
  one-shot descriptor immediately after its role, and retains only the
  publication endpoint long term. dbus-daemon inherits only its exact
  address-reporting descriptor for that bounded setup role and no publication
  endpoint. The target and descendants inherit no internal descriptor; other
  helpers retain no unrelated descriptor after their bounded setup role.
  `close_range(..., CLOEXEC)` remains a backstop, not the ownership proof.
- Replace required secretsd `--state-path` with required private
  `--publication-fd`. Remove production `libsecret-state.jcs` construction,
  pathname atomic replacement, host pathname reads, `exists`/parse fallback,
  and Checkout fallback after a final-state read failure. The bus tmpfs remains
  ephemeral and target-owned only for its target-facing bus/log role.
- Define a private, versioned publication framing module distinct from generic
  `underwrap_protocol::ipc::MAX_FRAME_SIZE`. A request contains exact magic,
  version, `PUBLISH` tag, big-endian `u64` attempt generation, big-endian `u32`
  body length, and exactly that many bytes. A response contains the same
  magic/version, exact `ACK` or `REJECT` tag, and the matching generation.
- Cap the private body inclusively at 1,048,576 bytes. Reject an over-cap length
  before body allocation. Generation zero is mandatory; every actual send
  attempt consumes the next exact generation, including a complete semantic
  rejection. Replay, gap, or overflow poisons the stream.
- Read fixed headers and bodies incrementally. A complete correctly framed body
  that fails canonical or semantic validation receives exact `REJECT(n)` and
  leaves the previously selected generation unchanged. Bad magic/version/tag,
  over-cap declaration, replay/gap/overflow, truncation, or transport failure is
  not resynchronized: poison the stream, stop accepting later candidates, close
  or shut down the response direction so secretsd fails instead of waiting
  forever, and boundedly drain/discard until terminal ownership is known.
- Do not withhold a response for a complete semantic rejection. secretsd accepts
  only the exact complete generation-matched response. `ACK` returns success;
  `REJECT`, mismatched/malformed response, EOF, or I/O failure returns D-Bus
  `Failed` where observable. Diagnostics carry generation/reason metadata only.
- Preserve the previous ACK-written bytes until a successor is fully read and
  validated. Completely write `ACK(n)` before marking candidate `n` latest.
  Partial/failed ACK keeps the previous generation, poisons the response path,
  and makes no claim that the D-Bus caller observed failure or success.
- Publish and synchronously host-validate generation zero before secretsd
  signals readiness. Initial rejection/channel failure makes secretsd fail before
  readiness; PID 2 reports controlled pre-exec failure, the target does not run,
  and F-006 permits neither `Checkin` nor `UsageSignal`.
- Keep the existing one-shot initialization and current secretsd store lock
  ordering. Every mutation canonicalizes/publishes while its existing write lock
  is held and waits for host response before D-Bus success. Do not add
  clone/publish/commit or in-memory rollback: a rejected publication may remain
  visible in secretsd memory and poison later attempts until F-012.
- In the host, synchronously receive and decide generation zero, then call the
  existing exact blocking F-006 gate observer unchanged. Generation-zero
  readiness ordering means no target mutation can publish while that tiny gate
  stream is pending.
- For libsecret-enabled runs, replace only the host's blocking relay wait with
  one bounded-cadence nonblocking loop that drains publication input, flushes
  exact pending responses, and calls `waitpid(relay, WNOHANG)`. Keep the host's
  sole inner-liveness writer through the loop. Non-libsecret runs retain the
  existing blocking wait path. Add no privileged receiver thread.
- A valid bounded generation must make progress under socket backpressure while
  the host drains and PID 1 independently polls liveness/reaps PID 2. No timeout
  or progress guarantee is made for a target that deliberately stalls a D-Bus
  call while it remains alive; target-caused denial of service remains out of
  scope.
- After relay reap, drop the inner liveness writer exactly as F-007 requires and
  perform one final nonblocking publication drain. Idle EOF is the required
  terminal ownership result. EOF mid-frame records a failed attempt but permits
  the prior ACK-written generation. `EAGAIN`/`WouldBlock` means a publisher
  endpoint remains open: return without waiting, collection, either terminal
  verb, or fallback.
- Parse the existing exact direct-target status after clean relay reap. A
  libsecret resource can enter collection/terminal dispatch only with all four
  facts: exact F-006 gate acceptance, one valid direct-PID-2 status, clean PID-1
  reap, and terminal publication EOF with at least ACK-written generation zero.
  Publication never authorizes dispatch on its own.
- Carry the selected supervisor-owned validated section into `WritebackInput`
  instead of a path. Make strict validation unskippable through a private
  validated-generation type or an exact final recheck. Compose only that section
  into the typed `ResourceBlob`; preserve env and files under their current
  authorities.
- Share a narrow libsecret semantic validator between secretsd publication and
  host acceptance. Validate:
  - exact canonical bytes by parsing and requiring byte-identical JCS re-encoding;
  - collection ids: nonempty, at most 128 bytes, ASCII alphanumeric or `_`;
  - item ids: exactly 32 lowercase hexadecimal bytes;
  - alias names: the same bounded valid single-segment rule;
  - alias targets: valid ids naming existing collections;
  - exact derived collection/item/alias path segment shape;
  - every collection/item `created` and `modified`: at most `2^53 - 1`;
  - aggregate canonical section bytes: at most 1,048,576.
- Keep arbitrary collection/item labels, content types, attribute keys/values,
  and secret bytes. Do not impose created-before-modified, future-time,
  collection-vs-item timestamp, or collection-modified propagation rules.
- Keep current policy/finalization behavior: unchanged state sends one
  `UsageSignal`; changed read-only state is dropped with current warning/audit
  and sends `UsageSignal`; changed writable state attempts one `Checkin`; the
  backend rechecks permission/writability; finalization errors remain
  diagnostic-only and subordinate to valid target status.
- Change the host target-fsuid collection window from files-or-libsecret to files
  only. This is direct deletion of obsolete pathname access, not a general
  capability-lifetime redesign. Combined runs still use the existing window for
  files.
- Update present-state architecture, security, secretsd security, libsecret
  task guidance, testing documentation, and affected Nix/VM comments. Describe
  the private capability, generation-zero readiness ACK, per-mutation ACK/REJECT,
  strict validation, latest-ACK-written fallback, final EOF/EAGAIN distinction,
  four-way lifecycle conjunction, response-loss window, policy behavior, and
  current 64-KiB generic-channel limitation without ticket/history language.

## Non-goals

- Do not change generic resource-channel framing, `ipc::MAX_FRAME_SIZE`,
  Checkout/Checkin sizing, backend/admin acceptance, or claim durable 1-MiB
  resource support (F-009).
- Do not add clone/publish/commit, rollback, or a side-effect-free D-Bus failure
  guarantee. Rejected mutations may remain in secretsd memory and poison later
  whole-state publication (F-012).
- Do not add handler-level `SetAlias` pre-mutation validation, exact
  `InvalidArgs`, alias object-registration repair, or in-memory no-side-effect
  semantics. Host validation narrows F-045's backend-persistence path but does
  not resolve F-045.
- Do not change wrong-typed `CreateItem` Label/Attributes handling; semantically
  valid empty values remain indistinguishable at whole-state validation (F-046).
- Do not update `Collection.Modified` from item mutations or add temporal-order
  consistency (F-039).
- Do not make generic `ResourceBlob::{to,from}_canonical_bytes` close all
  canonical/safe-integer ingress. This ticket narrows F-038 only for selected
  published libsecret generations.
- Do not change fd-20 authentication, fixed fd-21 liveness ABI, the F-006 gate
  protocol, target-status frame, PID-1/PID-2 topology, `ExecBundle`, env/files
  ownership, permission modes, writable checks, backend policy, last-writer-wins
  coordination, finalization-error precedence, or exact target status.
- Do not add a dependency, new binary/service/package, public API/config/wire or
  storage schema, privileged receiver thread, `SOCK_SEQPACKET`, memfd/SCM_RIGHTS
  generation transfer, preopened state file, public pathname, backend abort/CAS,
  cgroup/pidfd design, timeout policy, or target behavioral restriction. The
  existing `underwrap-exec → underwrap-ns` binary chain remains unchanged.
- Do not treat EOF as lifecycle authorization by itself. Do not use a latest
  generation after final `EAGAIN`, invalid gate/status/relay evidence, or before
  clean PID-namespace teardown.
- Do not claim exactly-once D-Bus responses, backend persistence on host ACK, or
  prevention of deliberate target-caused resource exhaustion/stalls.

## Acceptance criteria

- Static and production-path inspection prove there is no production
  `--state-path`, `libsecret-state.jcs`, libsecret final-state pathname reader,
  target-fsuid libsecret read, or Checkout fallback. The old target-writable
  pathname is irrelevant to selected state.
- Red-before evidence proves the current same-UID pathname replacement reaches
  host selection. After the fix, a live target's canonical, noncanonical,
  oversized, and semantically invalid old-path writes affect neither selected
  generation nor exact backend/audit result.
- The private frame codec accepts exact sequential generation 0 and later
  attempts; handles fragmented header/body; accepts cap-1 and cap; rejects cap+1
  before body allocation; and rejects bad magic/version/tag, replay/gap/overflow,
  truncation, empty/invalid bodies, read errors, and trailing protocol state
  without unbounded allocation or hangs.
- Exact response tests cover ACK, REJECT, wrong generation/tag/version, partial
  response, write/read failure, and EOF. A candidate becomes latest only after
  complete ACK write. Complete semantic rejection consumes its generation but
  never replaces the prior accepted bytes.
- Strict canonical tests reject whitespace, alternate ordering/escaping,
  unknown/defaulted fields, and duplicate keys through exact parse/JCS byte
  equality. Exact canonical bytes round-trip.
- Semantic tests cover empty/too-long/bad collection and alias segments; item id
  short/long/uppercase/nonhex/separator cases; dangling aliases; exact derived
  paths; timestamp max accepted and max+1 rejected. No temporal-order rule is
  asserted.
- Synchronized production tests prove generation-zero readiness waits for exact
  host ACK; a mutation D-Bus reply waits for exact ACK; REJECT/channel failure
  returns `Failed`; and host ACK followed by secretsd death before caller reply
  leaves the ACK-written generation host-eligible. Tests use barriers or exact
  protocol state, not timing guesses.
- Previous-generation tests ACK generations 0 and 1, then separately exercise
  semantic REJECT, partial frame plus EOF, publisher death, response failure plus
  EOF, and response loss. With exact gate/status/reap plus terminal EOF,
  generation 1 alone is selected; no partial/rejected bytes are used.
- A focused production-helper test deliberately retains an out-of-namespace
  publisher endpoint after relay reap. Final nonblocking drain returns `EAGAIN`
  without hanging and sends zero collection, `Checkin`, or `UsageSignal` bytes;
  no fallback is used and valid direct target status remains propagatable.
- Backpressure tests send one near-1-MiB generation and repeated generations
  whose aggregate exceeds socket capacity through the real host drain loop.
  Publication completes without socket-buffer deadlock while PID 1 retains its
  independent active liveness/reap behavior. No >64-KiB backend Checkin success
  is asserted.
- Descriptor tests cover socket creation, CLOEXEC, fd 3/20/21 collision safety,
  the one reviewed secretsd-exec inheritance exception, immediate CLOEXEC
  restoration, every fork/error branch, and final ownership. Deployed `/proc`
  evidence proves host-only receiver and secretsd-only long-lived publisher;
  PID 1, target PID 2, descendants, dbus-daemon, and helpers retain neither end
  or another internal authority fd.
- A production lifecycle matrix covers gate accepted/failed/malformed, target
  status valid/invalid, relay clean/unclean/wait failure, publication initial
  failure/EOF/EAGAIN/no-ACK generation. Libsecret collection and terminal
  dispatch occur only for all four valid facts. Valid target process status can
  propagate when publication suppresses resource dispatch.
- Initial publication rejection/failure is controlled pre-exec failure: target
  does not run and no terminal resource verb is sent. Existing F-006 invalid
  cells and F-007 synchronized stage-1/host/relay cancellation remain no-terminal
  and tear down every exact process/namespace identity.
- Healthy exit 0, exit 37, direct target signal, unchanged, changed writable,
  changed read-only, and policy-flip paths preserve exact target status,
  `UsageSignal`/`Checkin`/drop/audit behavior, and representative files plus
  libsecret state. Env and untouched typed slots remain byte-preserved.
- Libsecret-only host supervision performs no target-fsuid collection window;
  files-only and combined collection retain the existing bounded window.
- VM evidence covers the live same-UID old-path attack, legitimate D-Bus
  persistence, exact publication and internal-fd ownership, near-cap publication
  without a backend-size claim, F-007 cancellation regressions, policy/status
  behavior, and complete audit records. Plaintext and exact base64 fixture values
  never appear in component journals.
- Published documentation describes only the corrected present-tense lifecycle,
  trust boundary, failure semantics, response-loss window, and F-009 limitation.
  It makes no F-012 rollback, F-039 temporal consistency, F-045 handler, F-046
  variant, exactly-once, or durable-ACK claim.

## Security boundary

- Protected asset: the canonical, bounded, semantically valid libsecret object
  graph eligible for composition into the durable backend resource, including
  secret values, generated collection/item identity, exact path segments, alias
  referential integrity, and safe timestamps.
- Attacker: the configured target and descendants. They are trusted to receive
  and use their projection but their D-Bus and filesystem input is untrusted for
  storage integrity and robustness. Root, kernel, backend, root-owned
  configuration, secretsd code, and host supervision remain trusted.
- Boundary direction: untrusted target input must flow `target → private D-Bus →
  secretsd canonical publication → trusted host validation/retention → existing
  policy-gated fd 20 finalization → backend`. A target-writable filesystem entry
  must never flow directly into trusted host selection.
- Invariant: a libsecret terminal resource verb requires exact F-006 `Armed`, one
  valid direct-PID-2 status, clean PID-1 reap, terminal nonblocking publisher EOF,
  and at least generation zero whose complete bytes passed the host's exact
  size/canonical/semantic validation and whose exact ACK was completely written.
  No partial/rejected bytes, pathname, Checkout fallback, or still-open publisher
  can satisfy this conjunction.

## Adjacent findings

- F-009 remains separate and unchanged: private publication is capped at 1 MiB,
  while generic Checkout/Checkin remains 64 KiB. Host ACK is not durability.
- F-012 remains separate and unchanged: host retention is atomic, but rejected
  mutation state can remain in secretsd memory.
- F-038 is affected/narrowed, not resolved: selected published libsecret
  timestamps become safe-integer bounded; generic resource ingress remains.
- F-039 remains separate and unaffected: stale collection modification time is
  accepted and no temporal rule is added.
- F-045 is affected/narrowed, not resolved or prerequisite: malformed/dangling
  aliases cannot replace host-selected backend state, but handler-level
  pre-mutation validation, exact error, poisoned memory, and object registration
  remain later work.
- F-046 remains separate and unchanged: whole-state validation cannot detect the
  wrong-typed-input history behind semantically valid empty properties.

## Validation

- `cargo test -p underwrap-protocol libsecret_ -- --nocapture`
- `cargo test -p underwrap-protocol`
- `cargo test -p underwrap-secretsd-store`
- `cargo test -p underwrap-secretsd publication_ -- --nocapture`
- `cargo test -p underwrap-secretsd`
- `cargo test -p underwrap-ns publication_ -- --nocapture`
- `cargo test -p underwrap-ns lifecycle_conjunction_ -- --nocapture`
- `cargo test -p underwrap-ns`
- `cargo test -p integration-tests`
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `nix build .#checks.x86_64-linux.vm-test`
- Focused VM evidence for the red-before/live old-path attack, legitimate D-Bus
  persistence, exact publication/internal-fd ownership, near-cap backpressure,
  current policy/status behavior, F-007 cancellation, complete audit records,
  and plaintext/base64 log absence.
- Static production/documentation search proving no `--state-path`,
  `libsecret-state.jcs`, or final-state pathname consumer remains.
- `git diff --check`

No web or browser check is required because this task changes no SPA surface.
Run resource-intensive checks and subagent work sequentially with
`CARGO_BUILD_JOBS=2`. Python validation uses `PYTHONDONTWRITEBYTECODE=1` and must
leave no `__pycache__` or `.pyc` artifact. New Rust files must be exposed to the
Git-filtered Nix source only through the exact approved path allowlist or
intent-to-add while validating.

## Dependencies

- Completed prerequisite:
  `tickets/03_done/1050_task_prevent_pre_exec_partial_writeback.md` (F-006;
  final fix `5b394e9c22754afb39bcb08c8eff8df77ba4995f`).
- Completed prerequisite:
  `tickets/03_done/1060_task_prevent_supervisor_termination_from_orphaning_targets.md`
  (F-007; final fix `4502f6f80fe45c5e2ce5ce6fa29015459f9cf3bb`).
- No unresolved implementation dependency. F-009, F-012, F-038, F-039, F-045,
  and F-046 remain separately gated as described above.
