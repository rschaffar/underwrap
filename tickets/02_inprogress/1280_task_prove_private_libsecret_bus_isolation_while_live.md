# Prove private libsecret bus isolation while live

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-029
Source themes: C-007

## Outcome

The deployed NixOS test proves that a same-caller host process cannot see or
connect to the namespace-private libsecret filesystem socket while the wrapped
target, D-Bus daemon, and Secret Service are demonstrably usable. Teardown can
no longer make the isolation assertion pass vacuously.

## Scope

- Add one dedicated test-only libsecret target and wrapper fixture. The target:
  - completes a real lookup against the configured resource;
  - emits only a fixed readiness marker and its non-secret
    `DBUS_SESSION_BUS_ADDRESS`;
  - blocks on an explicit host-controlled release signal rather than elapsed
    time; and
  - completes a second real lookup and emits a fixed post-probe marker before
    exiting.
- Replace all three current background-short-lookup/sleep bus-invisibility
  probes with one authoritative live witness in the VM test.
- Parse and validate the target-reported `unix:path=` address as a pathname
  beneath `/tmp/.underwrap-ns-bus/` before using it as evidence.
- While the exact wrapper process is still live and before releasing the target,
  prove from a separate process running as the original caller that:
  - the exact private socket pathname is absent in the host mount namespace;
  - the private bus tree exposes no socket; and
  - an exact `AF_UNIX` connection attempt to that pathname fails with
    `ENOENT`, not merely with a generic failure.
- Launch the wrapper as the original caller in a uniquely named transient
  systemd unit/cgroup. Require that exact unit to remain active across the host
  probes, release the target only through a unique explicit channel/marker,
  require its second successful-lookup marker and clean unit exit, and stop only
  that unit/cgroup during bounded failure cleanup.
- Update `docs/testing.md` with the live-before/live-after, path, connection,
  and teardown-resistant evidence boundary.

## Non-goals

- Do not change production namespace creation, mount propagation, bus paths,
  D-Bus/secretsd startup, publication, target lifecycle, permissions, target
  identities, or the `underwrap-exec → underwrap-ns` chain.
- Do not add a dependency, production test hook, config option, alternate bus
  mode, broad process-name kill, or timing-only readiness assertion.
- Do not print, copy, persist, or search for the looked-up secret value. The
  probe target discards lookup output; only fixed markers and the private bus
  address may cross its stdout.
- Do not claim isolation from root or the kernel, hide abstract Unix sockets,
  treat the target as adversarial, or expand the existing filesystem-session-bus
  guarantee.
- Do not absorb F-030 assertion matrices, F-032 unsafe rationale, F-038/F-040/
  F-045 Secret Service behavior, U-items, or any Opportunity.
- Do not weaken or remove existing libsecret operation, target-UID, files-mount,
  lifecycle, publication, secret-hygiene, or cleanup assertions to make the new
  witness pass.

## Acceptance criteria

- No bus-invisibility assertion is preceded only by a short background lookup
  and sleep; the three vacuous probes are absent.
- One configured test-only target proves Secret Service usability immediately
  before blocking and immediately after release with two successful lookups,
  without emitting the returned secret.
- A fixed readiness marker supplies one validated filesystem D-Bus address, and
  the uniquely named transient unit/cgroup for that wrapper invocation is active
  immediately before and after the host isolation probes.
- During that live interval, the same-caller host view has no socket at the exact
  private pathname or elsewhere below `/tmp/.underwrap-ns-bus/`, and an exact
  Unix-stream connection returns `ENOENT`.
- The target is released only after all host probes through a unique explicit
  channel/marker, emits the fixed post-probe marker only after the second
  lookup, and its transient unit exits cleanly; bounded failure cleanup targets
  only that exact unit/cgroup and cannot signal a reused PID or kill unrelated
  wrappers.
- Existing libsecret store/lookup/writeback, mandatory target-UID isolation,
  files projection, F-006/F-007 lifecycle, F-008/F-012 publication semantics,
  terminal audit behavior, and secret-log hygiene remain green.
- Documentation states what the live VM witness proves and preserves the
  filesystem-socket/abstract-socket, trusted-target, root, and kernel limits.
- Only the VM fixture/test and testing documentation change. No production,
  dependency, manifest/lock, module, protocol/API, privileged-chain, F-030+, or
  Opportunity change is introduced.

## Security boundary

- Protected asset: brokered libsecret data and the invocation-private D-Bus
  endpoint that carries Secret Service requests and responses.
- Attacker: another untrusted host process owned by the original wrapper caller,
  probing while the trusted configured target is using its projection.
- Boundary direction: same-caller host mount namespace → target-private mount
  namespace and filesystem Unix socket.
- Invariant: `docs/security.md` states that private mount propagation keeps
  files/libsecret mounts out of the host mount namespace. Evidence must hold
  while the endpoint is active; post-teardown absence is not evidence.

## Validation

- Preserve the red/static baseline: three `setsid ... secret-tool lookup`
  invocations finish normally before their one-second-delayed host probes, and
  there is no target readiness, live-process, exact-address, or connection
  witness.
- Re-run the deterministic desired-evidence inventory and require every live
  witness condition to pass.
- Parse `nix/tests/test.py` as Python without leaving bytecode.
- `nix build --no-link .#checks.x86_64-linux.vm-test`
- `nix flake check --all-systems --no-build`
- `nix flake check`
- `git diff --check`
- Static inventory: one authoritative live bus witness; no short-lookup/sleep
  probes, broad `pkill`, secret output, production/module/dependency/lock change,
  generated output, or retained test process.

## Dependencies

- F-007 provides fail-closed target/namespace teardown; this task proves the bus
  boundary while that lifecycle is still active and does not change it.
- F-008/F-012 provide host-validated Secret Service publication/live-state
  semantics; the two probe lookups consume those existing semantics without
  changing them.
- F-028 is complete and automatically accepted at
  `a7ee4c410f3f22ba988c652988cc6c268bd06d7f`; its hardened wrapper-auth witness
  remains unchanged.
- D-174 authorizes one-ticket-at-a-time continuation subject to all stop,
  cleanup, review, validation, and transition rules.
- F-030 and later findings remain separate.
