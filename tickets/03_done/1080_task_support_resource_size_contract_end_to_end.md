# Support the resource size contract end to end

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-009
Source themes: C-003

## Outcome

Every accepted resource has one explicit end-to-end size contract that survives
its actual encodings and every supported hop. Approval of this exact ticket
explicitly selects an inclusive maximum of 2,097,152 bytes (2 MiB) for the
complete JCS-canonical `ResourceBlob`, distinct from and in addition to the
preserved 1,048,576-byte (1-MiB) invocation-private libsecret publication-body
limit. This deliberate first-release contract extends beyond F-009's original
1-MiB recommendation and is not authorized until ticket-specific user approval.
It keeps the documented full-size libsecret section usable inside a complete
resource instead of reducing it to the old accidental local-frame ceiling.

A complete canonical resource at or below 2 MiB can pass backend persistence,
base64/JSON runtime transport, daemon admission, authenticated fd-20 Checkout,
and fd-20 Checkin. The exact wire envelope is budgeted in addition to blob
bytes: `Checkin` carries a one-byte tag plus a four-byte vector length, while
`CheckoutOk` additionally carries the one-byte writable flag. The four-byte
frame length prefix is not part of the payload maximum. A blob one byte over the
resource maximum is rejected as resource data even if its smaller request
envelope would otherwise fit the larger response-oriented frame bound.

The 64-KiB generic control-frame limit remains in force for wrapper selectors,
execution metadata, and other small IPC. Brokered env values stop being copied
through `WrapperResponse::ReadOk` and fd 3 solely to work around that control
path: those internal messages carry only the operator-owned env-projection flag,
and `underwrap-ns` obtains and validates env values from the already authenticated
complete Checkout blob before any projection effect. Bulk resource frames alone
use the separately bounded resource limit.

Backend/admin admission rejects a would-be complete resource over 2 MiB before
success or persistence, preserving the previous blob. Runtime open never returns
an oversized resource; runtime finalize returns a stable invalid-blob size reason
without saving it. Resource-bearing HTTP requests receive explicit base64/JSON
headroom without raising the existing limit for unrelated requests. The daemon,
wrapper chain, and host publication receiver independently fail closed on an
oversized or malformed peer response.

For libsecret publication, the host writes ACK only after the exact section is
valid under F-008 and its composition with the immutable Checkout env and
Checkout files is at most 2 MiB. This makes ACK establish transport-size
eligibility for that candidate; it remains neither D-Bus caller receipt nor
backend durability. A target may still make later files changes that make the
final combined resource oversized; direct writable-file staging has no
per-mutation acknowledgement, and the existing finalization-error/status rule
continues to reject that final composition without replacing valid target
status.

This is a first-release correctness/security correction with an explicit
product-contract choice, not a compatibility migration. Sizes between the
accidental 64-KiB transport ceiling and the selected 2-MiB resource contract
become usable. Larger state that could previously be stored through some admin
paths but could not complete a wrapper round trip now fails early and explicitly.

## Scope

- Define one shared inclusive `MAX_CANONICAL_RESOURCE_BYTES = 2 * 1024 * 1024`
  authority beside `ResourceBlob` and use it for complete canonical resource
  bytes. Keep `libsecret_publication::MAX_BODY_BYTES = 1_048_576` as a distinct
  section/publication limit.
- Make `ResourceBlob::from_canonical_bytes` reject an over-limit input before
  JSON parsing and make canonical serialization reject an over-limit result.
  Preserve schema version 1, exact JCS bytes where required, and all existing
  typed ownership/canonical semantics.
- Enforce the same complete-blob limit before every `StorageBackend` resource
  write. An oversized save attempt must be rejected before invoking the storage
  write and must leave the prior generation byte-identical. Loading a directly
  injected/corrupt oversized
  stored blob fails closed instead of returning it to runtime.
- Give operator-originated env/files/libsecret mutations a stable HTTP 400
  `resource_too_large` result when their composed blob would exceed the limit.
  Cover flat write and rename plus structured libsecret mutations that can add
  material; deletion remains allowed when it reduces valid state. Preserve
  current audit result labeling and no-secret diagnostics.
- Give runtime finalize a stable `InvalidBlob` reason
  `resource-too-large`. Reject based on decoded canonical bytes before parsing,
  re-encoding, env comparison, persistence, or `last_used_at` success handling.
  Runtime open must not return `Allow` for an oversized stored blob.
- Add a resource-bearing HTTP body bound with enough explicit base64 and JSON
  envelope headroom for a 2-MiB canonical blob and a one-byte-over semantic
  rejection vector. Do not increase unrelated bootstrap, login, policy, or
  ordinary control-request limits. Check length before unbounded body growth.
- Keep the existing 64-KiB `ipc::MAX_FRAME_SIZE` for small control messages.
  Add explicit sync and async bounded-frame entry points (or an equivalently
  auditable typed mechanism) used only by the wrapper-resource channel.
- Set the wrapper-resource frame payload maximum to exactly
  `MAX_CANONICAL_RESOURCE_BYTES + 6`: one response tag, one four-byte blob
  length, and the writable flag. The four-byte outer length prefix remains
  separately read and checked before payload allocation. `Checkin` uses five
  bytes of envelope; the shared six-byte channel bound must not let a
  `MAX + 1` Checkin through message-level validation.
- Apply the bulk bound on both ends of the authenticated resource socketpair:
  daemon async read/write and namespace sync read/write. Keep wrapper request,
  wrapper response, fd-3 `ExecBundle`, lifecycle, status, publication, and all
  other local protocols on their narrower independent bounds.
- Remove brokered env material from `WrapperResponse::ReadOk` and `ExecBundle`.
  Carry only whether env projection is configured, thread that trusted flag into
  `ExecResourceBinding`/`ResourceChannel`, and obtain resource env values from
  the complete fd-20 Checkout blob in `underwrap-ns`.
- Preserve daemon-side full-blob parsing and configured-env-name validation.
  Independently validate selected env names before namespace preparation,
  projection staging, target-facing helper launch, or target execution, and
  construct the target environment from the same decoded Checkout blob. Preserve
  the existing authorized secret-free idmap-helper ordering before fd 3 and
  Checkout; that helper closes secret-bearing descriptors and is not moved behind
  Checkout. Preserve
  `env_clear`, precedence, allowlists, immutable-env finalize checks, and F-044's
  still-separate forced-XDG issue.
- Before any F-008 generation ACK, validate the `LibsecretSection` exactly as
  today, compose it with the Checkout resource's immutable env and current
  Checkout files, canonicalize it under the 2-MiB complete-resource authority,
  retain it, then completely write ACK. A size rejection uses the existing
  exact generation-matched REJECT path and does not replace the latest
  ACK-written generation.
- Preserve F-008 generation-zero-before-readiness, ACK-before-selection,
  terminal-EOF fallback, final-EAGAIN failure, response-loss window, and
  gate/status/reap/publication conjunction. Preserve F-006/F-007 no-terminal
  failure behavior and exact target exit/signal status.
- Make end-of-run composition use the same complete-resource size authority.
  Oversized direct files changes or combined final state produce no Checkin;
  finalization failure remains diagnostic and does not replace a valid target
  status. Do not convert ACK or writable staging into a durability promise.
- Update current-state backend, architecture, security, Secret Service, wrapper,
  and testing documentation. State the 2-MiB complete resource cap, independent
  1-MiB publication cap, base64/wire-envelope accounting, control-versus-bulk
  bounds, env's fd-20-only secret path, stable oversize rejection, and exact ACK
  limitation without ticket/history language.
- Capture deterministic red-before evidence against clean baseline
  `c25979d3354ff50ea6cd634d82e99cff515e0178` outside the working tree before
  implementing the correction.

## Non-goals

- Do not reduce the independent 1-MiB libsecret publication-body capacity or
  reinterpret it as including the complete `ResourceBlob` envelope.
- Do not support a complete canonical resource above 2 MiB, add a configurable
  size option, retain a compatibility exception, or migrate/truncate an existing
  oversized resource. Directly injected oversized storage fails closed and can
  be removed through trusted whole-resource administration/recovery.
- Do not globally raise the 64-KiB control-frame allocation bound. Do not make
  arbitrary root-owned configuration, wrapper selectors, argv, fd-3 metadata,
  error text, lifecycle frames, or public request DTOs bulk-sized.
- Do not add chunking, compression, streaming HTTP, `memfd`, `SCM_RIGHTS` blob
  transfer, `SOCK_SEQPACKET`, a new fd, a new process/thread/service/binary, or a
  new dependency.
- Do not change the storage schema/version, key layout, runtime/admin JSON
  shapes, Nix options, public configuration, permission model, mTLS principal,
  backend policy authority, last-writer-wins coordination, or
  `underwrap-exec -> underwrap-ns` binary chain. The only new public wire values
  are the stable oversize error/reason strings within existing error envelopes.
- Do not make Secret Service mutation failure side-effect free or roll back a
  rejected in-memory mutation (F-012). A complete-size REJECT can still leave
  the attempted state in secretsd memory and poison later attempts.
- Do not change collection/item timestamps (F-039), alias handler semantics
  (F-045), wrong-typed variants (F-046), or generic safe-integer handling
  (F-038).
- Do not fix libsecret-only resource deletion/orphan handling (F-011), resource
  transaction/CAS behavior, concurrent admin/runtime edits, channel
  serialization/stall policy (F-017/O-009), or finalization-error shell-status
  precedence (O-011).
- Do not reduce the 8-MiB files staging tmpfs or promise that every target-local
  file write persists. A final combined blob over 2 MiB is invalid resource
  state and follows the existing diagnostic-only finalization failure rule.
- Do not remove daemon full-blob parsing or claim resolution of F-042. Do not
  absorb F-043 protocol/vocabulary cleanup merely because local message fields
  change for this correction.
- Do not expose blob bytes, secret values, base64 material, canonical payloads,
  or credentials in errors, logs, tickets, tests, or validation artifacts.
- Do not weaken fd-20 authentication, dedicated target identity, fd closure,
  F-006 authorization, F-007 active liveness/status, F-008 publication
  ownership, capability/drop, namespace, core, dumpability, or environment
  protections.

## Acceptance criteria

### Authoritative resource limit

- One documented protocol constant defines the inclusive 2,097,152-byte limit
  for complete JCS-canonical `ResourceBlob` bytes. It is not inferred from the
  HTTP limit, frame prefix, base64 length, tmpfs size, or private publication
  body cap.
- Exact canonical blobs of `MAX - 1` and `MAX` bytes serialize/parse/save/load;
  `MAX + 1` is rejected before parse or persistence. Tests use real
  `ResourceBlob` values tuned to the exact canonical byte counts, not an
  unrelated byte vector or a test-only higher cap.
- Oversize serialization, parse, load, save, runtime open, runtime finalize,
  and admin mutation diagnostics identify size and the public maximum without
  including content. Existing stored bytes are byte-identical after every
  rejected mutation/save.
- The independent libsecret section body accepts its existing cap-1/cap vectors.
  A maximum-size exact section inside otherwise small env/files state can still
  be host-ACKed, checked in, and checked out on the next invocation.

### HTTP and backend admission

- Resource-bearing HTTP extractors accept a request carrying an exact 2-MiB
  canonical blob after base64/JSON encoding and allow a `MAX + 1` body to reach
  semantic size rejection. They reject a body above their explicitly bounded
  encoded envelope before unbounded allocation. Unrelated request extractors
  retain their existing 2-MiB bound.
- Flat env/files write and rename plus each structured libsecret material-adding
  path return HTTP 400 with exact code `resource_too_large` when the post-
  mutation complete blob is over the limit; they return no success, make no
  storage write, and preserve the prior typed blob. Delete/reducing operations
  keep their successful behavior.
- Runtime finalize decodes base64, detects complete canonical bytes over the
  limit before JSON work, returns exact `InvalidBlob { reason:
  "resource-too-large" }`, does not save, and retains existing invalid-finalize
  `last_used_at` behavior. A corrupted/directly injected oversized resource
  makes runtime open fail closed before `Allow` and before any local channel is
  created.
- Backend store helpers enforce the same limit for plain and encrypted stores;
  no alternate admin helper can bypass it. Tests cover direct helper calls,
  representative real handlers/routes, and runtime handlers rather than only a
  protocol utility.

### Local framing and message ownership

- Default sync/async frame APIs still accept at most 65,536 payload bytes and
  reject a larger declared prefix before allocation. Resource-channel APIs
  accept at most `2,097,152 + 6` payload bytes and reject a larger declared
  prefix before allocation.
- Exact payload accounting is pinned:
  `Checkin(MAX) == MAX + 5`, `CheckoutOk(MAX) == MAX + 6`, and the outer frame
  adds exactly four bytes. Both exact-max messages round-trip synchronously and
  asynchronously. `Checkin(MAX + 1)` is rejected by the message/resource bound
  even though its payload equals the shared response-oriented channel maximum;
  `CheckoutOk(MAX + 1)` is rejected by the frame bound.
- Malformed/truncated prefixes, declared over-limit payloads, shorter/longer
  blob vector lengths, trailing bytes, read errors, and write-side oversize all
  fail deterministically. Tests do not allocate or send the attacker-declared
  body after an over-limit prefix.
- Only the scoped wrapper-resource channel opts into the bulk bound. Static
  call-site inventory and negative tests prove wrapper intake/response, fd 3,
  lifecycle, target status, publication, and all other IPC remain at their
  current independent limits.

### Env projection path

- `WrapperResponse::ReadOk` and `ExecBundle` contain no brokered env map or env
  secret bytes. Their wire tests and production call sites carry only the env-
  projection selector plus existing trusted metadata. The scoped fd-20 Checkout
  is the only local transport of complete resource env values into
  `underwrap-ns`.
- The daemon still parses the complete backend blob and validates configured env
  names before granting. `underwrap-ns` independently validates and selects env
  from the same canonical Checkout blob during complete projection planning.
  Invalid names fail before namespace preparation, projection staging,
  target-facing helper launch, or target execution and produce no terminal
  resource verb. The existing secret-free idmap helper remains before fd 3 and
  Checkout and retains its exact descriptor/dumpability boundary.
- A real env-only resource whose canonical blob and env payload are above the
  old 64-KiB ceiling reaches the configured target exactly. Existing
  passthrough/fixed/resource precedence, `env_clear`, no-`LD_*`, forced paths,
  binary selection, fd handoff, and exact exit/signal behavior remain.

### Libsecret and finalization

- Generation zero and later candidates are ACK-eligible only after existing
  strict F-008 section validation and a complete-resource size check using the
  Checkout env/files baseline. A too-large composition receives the exact
  generation-matched REJECT, does not become latest, and preserves the earlier
  ACK-written generation under the existing terminal-EOF rules.
- Synchronized tests prove a near-1-MiB valid libsecret section is no longer
  merely host-ACKed: with otherwise small state it crosses Checkin, backend
  finalize, subsequent Checkout, and D-Bus readback. The test retains and joins
  every responder/child and does not infer durability from ACK alone.
- Exact 1-MiB private publication plus complete-resource overhead is accepted
  when the total is at most 2 MiB. A candidate whose complete composition is
  exactly 2 MiB is size-eligible for ACK and, when all existing F-008 validation,
  lifecycle, and complete-response-write conditions succeed, receives ACK; one
  that makes it 2 MiB plus one byte receives the exact generation-matched REJECT
  for size before D-Bus success.
- Final files-only or combined collection at `MAX` can Checkin; `MAX + 1`
  sends no oversized frame and cannot persist. Existing F-006/F-007/F-008
  lifecycle conjunction and direct target status remain authoritative.
  Finalization failure remains diagnostic-only and O-011 remains separate.
- Unchanged, writable, read-only, policy-flip, exit-0, exit-nonzero, and signal
  paths retain their exact UsageSignal/Checkin/drop/audit/status behavior for
  valid-size state. F-008 final EAGAIN still authorizes no fallback, collection,
  Checkin, or UsageSignal.

### Red-before and deployed evidence

- Reproduce baseline `c25979d3354ff50ea6cd634d82e99cff515e0178` in a disposable
  external tree. Deterministically show: a resource just above the encoded
  64-KiB Checkout/Checkin ceiling cannot use the production channel; selected
  env above that ceiling cannot cross `ReadOk`/fd 3; an oversized complete blob
  can be saved through the current backend helper/admin composition; and F-008
  can host-ACK a near-1-MiB generation that the current Checkin frame cannot
  carry. Record exact encoded lengths and reap every child/thread/process.
- After the fix, focused production-path and real backend/daemon integration
  cases cover env, files, libsecret, Checkout, and Checkin below/at/above the
  chosen boundaries. No test relies only on constants, parser mocks, sleep-only
  timing, or the old 32,000-byte "large" case.
- The NixOS VM proves through the installed setgid/capability chain: an env
  resource over 64 KiB reaches the target exactly; a near-1-MiB D-Bus mutation
  persists and is readable on the next invocation; complete resources at the
  selected valid boundary round-trip; oversize admin/mutation attempts fail
  without changing the prior resource; and normal F-006/F-007/F-008
  cancellation/status/policy evidence remains.
- VM and focused logs are scanned for plaintext and exact base64 fixture forms.
  Size diagnostics contain identifiers, lengths, and reason codes only. Every
  process, responder thread, server, and QEMU instance is explicitly reaped or
  joined, including failure paths.
- Present-state docs and source contain no statement that generic resource
  Checkin remains 64 KiB or that host ACK can fail solely because the valid
  complete resource cannot fit the local bulk frame. They continue to say ACK
  is not caller receipt or backend durability.

## Security boundary

- Protected asset: availability and integrity of an authorized canonical
  resource and of a target mutation that is valid under the published size
  contract, without permitting allocation bombs on unrelated control channels.
- Attacker: target-controlled D-Bus/input state and malformed local
  wrapper-resource input. An authenticated operator may also create large valid
  state but is not treated as an attacker. Backend, root configuration, host
  root, and kernel remain trusted as defined in `docs/security.md`.
- Boundary direction: complete resource bytes flow
  `admin/StorageBackend -> runtime HTTPS/base64 -> underwrapd -> authenticated
  fd 20 Checkout -> underwrap-ns`, and changed bytes return through fd-20
  Checkin and runtime finalize. Libsecret input additionally flows
  `target -> private D-Bus -> secretsd publication -> host validation` before
  joining that complete resource.
- Invariant: every complete canonical resource accepted at or below 2 MiB can
  traverse each required encoded hop with its envelope still bounded; anything
  larger is rejected before admin success, publication ACK where the host can
  determine the composed size, local bulk allocation/persistence, or runtime
  `Allow`/finalize success. Small control protocols do not inherit the bulk
  allocation allowance.
- Required project invariants: fd 20 remains the first authenticated resource
  authority; env never enters argv/inter-stage environment/logs; complete
  pre-effect planning and exact F-006/F-007/F-008 terminal conjunction remain;
  backend permission/writable re-evaluation and last-writer-wins behavior remain.

## Adjacent findings

- F-008 is a completed prerequisite and regression surface. This ticket removes
  only its documented generic-size limitation and adds complete-resource size
  eligibility before ACK. It does not change publication ownership, generation,
  fallback, EAGAIN, or ACK/durability semantics.
- F-011 remains separate: whole-resource deletion still needs to account for
  libsecret-only dependents and retained blobs.
- F-012 remains separate: a rejected oversize publication can remain in
  secretsd memory; this ticket protects host selection/persistence, not handler
  rollback.
- F-017/O-009 remain separate: no timeout, queue, repeated-verb, idle-lifetime,
  or global daemon-concurrency policy is added.
- F-038, F-039, F-045, and F-046 remain separate and unchanged except that a
  complete oversize graph cannot be selected/persisted.
- F-042 remains separate: the daemon continues to parse the complete blob even
  though env material no longer crosses the small control messages.
- F-043 remains separate: changing the private env field shape does not approve
  scope/audit/helper cleanup.
- O-011 remains separate: local/final backend persistence failure does not
  replace the target's primary exit or signal status.

## Validation

Run resource-intensive checks sequentially with `CARGO_BUILD_JOBS=2`. Python
validation uses `PYTHONDONTWRITEBYTECODE=1`; no `__pycache__` or `.pyc` may
remain.

- Preserve deterministic red-before artifacts from clean
  `c25979d3354ff50ea6cd634d82e99cff515e0178` with exact encoded lengths and
  clean teardown.
- `CARGO_BUILD_JOBS=2 cargo test -p underwrap-protocol ipc -- --nocapture`
- `CARGO_BUILD_JOBS=2 cargo test -p underwrap-protocol`
- `CARGO_BUILD_JOBS=2 cargo test -p underwrap-backend-store`
- `CARGO_BUILD_JOBS=2 cargo test -p underwrap-backend resource_size -- --nocapture`
- `CARGO_BUILD_JOBS=2 cargo test -p underwrap-backend`
- `CARGO_BUILD_JOBS=2 cargo test -p underwrapd resource -- --nocapture`
- `CARGO_BUILD_JOBS=2 cargo test -p underwrapd`
- `CARGO_BUILD_JOBS=2 cargo test -p underwrap-exec`
- `CARGO_BUILD_JOBS=2 cargo test -p underwrap-secretsd`
- `CARGO_BUILD_JOBS=2 cargo test -p underwrap-ns resource -- --nocapture`
- `CARGO_BUILD_JOBS=2 cargo test -p underwrap-ns publication -- --nocapture`
- `CARGO_BUILD_JOBS=2 cargo test -p underwrap-ns`
- `CARGO_BUILD_JOBS=2 cargo test -p integration-tests`
- `CARGO_BUILD_JOBS=2 cargo test --workspace`
- `CARGO_BUILD_JOBS=2 cargo fmt --all -- --check`
- `CARGO_BUILD_JOBS=2 cargo clippy --all-targets --workspace -- -D warnings`
- `cd web && npm test && npm run build`
- Parse the deployed Python evidence without writing bytecode:

  ```bash
  PYTHONDONTWRITEBYTECODE=1 python3 - <<'PY'
  import ast
  from pathlib import Path

  for path in [Path("nix/tests/test.py")]:
      ast.parse(path.read_text(), filename=str(path))
  PY
  test -z "$(find . -path ./.git -prune -o \
    \( -name '*.pyc' -o -name __pycache__ \) -print)"
  ```

- Run the static, scope, secret-hygiene, and exact-index checks:

  ```bash
  rg -n 'MAX_FRAME_SIZE|MAX_CANONICAL_RESOURCE_BYTES|MAX_WRAPPER_RESOURCE_FRAME_SIZE' \
    crates/underwrap-protocol crates/underwrap-backend-store \
    crates/underwrap-backend crates/underwrapd crates/underwrap-exec \
    crates/underwrap-ns
  rg -n 'read_frame_(sync|async)|write_frame_(sync|async)' \
    crates/underwrap-protocol crates/underwrapd crates/underwrap-exec \
    crates/underwrap-ns
  git diff --check
  git diff --cached --check
  git diff --cached --no-renames --name-only | LC_ALL=C sort \
    > /tmp/underwrap-1080-staged-paths.actual
  LC_ALL=C sort /tmp/underwrap-1080-stage-allowlist.txt \
    > /tmp/underwrap-1080-staged-paths.expected
  cmp /tmp/underwrap-1080-staged-paths.expected \
    /tmp/underwrap-1080-staged-paths.actual
  test -z "$(git diff --name-only)"
  test -z "$(git ls-files --others --exclude-standard)"
  if git diff --cached --no-color -U0 | \
    rg 'BEGIN ([A-Z0-9 ]+)?PRIVATE KEY|AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9]{20,}'; then
      exit 1
  fi
  ```

  The controller must create `/tmp/underwrap-1080-stage-allowlist.txt` from the
  independently reviewed exact intended paths before staging; deriving it from
  the dirty tree is not allowed. No generated artifact may remain.
- `nix build .#checks.x86_64-linux.vm-test`
- `nix flake check`; if the known clean-baseline LLVM 21 `llvm-profdata merge`
  SIGILL recurs after coverage tests pass, preserve exact output and do not
  misattribute it to this ticket.

If an approved new Rust file is untracked while a Nix check runs, expose only
that file through the workflow's exact intent-to-add/staging rule or an explicit
path flake; never stage unrelated work.

## Dependencies

- Completed and completion-accepted prerequisite:
  `tickets/03_done/1070_task_replace_libsecret_state_path_with_validated_publication.md`
  (F-008; cleanup `235da4652b52a0754659e32c16c28df204dff18d`, fix
  `a1ee6cf1392a807fc88bc7a56347270c402e044c`). Its exact publication/lifecycle
  semantics are binding.
- Completed regression surfaces:
  `tickets/03_done/1000_task_preserve_untouched_resource_slots.md` (F-001),
  `tickets/03_done/1050_task_prevent_pre_exec_partial_writeback.md` (F-006), and
  `tickets/03_done/1060_task_prevent_supervisor_termination_from_orphaning_targets.md`
  (F-007).
- No unresolved implementation dependency. F-011/F-012/F-017/F-038/F-039/
  F-042/F-043/F-045/F-046 and O-009/O-011 remain separately gated.
- Mandatory repository-wide and ticket-local behavior-preserving cleanup
  discovery occurs only after this exact ticket is approved. No cleanup unit is
  pre-approved.
