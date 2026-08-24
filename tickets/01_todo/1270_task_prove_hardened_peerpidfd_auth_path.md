# Prove the hardened SO_PEERPIDFD authentication path

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-028
Source themes: C-007

## Outcome

The deployed hardened NixOS test positively witnesses that a successful wrapper
connection was authenticated through `SO_PEERPIDFD`, and deterministic daemon
unit tests prove that only kernel-reported option unavailability selects the
weaker `SO_PEERCRED + /proc` path. Every acquisition or resolution failure after
selecting the pidfd path rejects the peer without downgrade.

## Scope

- Add a private, testable peer-auth operation boundary around the existing
  `get_peer_cred_for_fd` decision flow while production continues to call the
  same `SO_PEERCRED`, `SO_PEERPIDFD`, pidfd-fdinfo, `/proc/<pid>/status`, and fd
  close operations.
- Add deterministic peer-auth tests covering:
  - `ENOPROTOOPT` classification as the sole unsupported/fallback result;
  - successful fallback with `PeerCredFallback` only after that result;
  - every other pidfd acquisition error rejecting without a fallback status
    read;
  - acquired-pidfd PID-resolution and status-read failures rejecting without
    fallback;
  - acquired-pidfd success producing `PeerPidfd`; and
  - exact acquired-pidfd close behavior on success and every failure path.
- Strengthen the existing real socketpair check on kernels that support
  `SO_PEERPIDFD` without making the daemon crate unsupported on an explicitly
  weaker older host.
- Add one deployed VM witness around a successful setgid wrapper invocation:
  its post-cursor daemon record contains `auth_path: PeerPidfd`, and no fallback
  warning/path appears for that invocation on the module-enforced Linux 6.5+
  hardened host.
- Update `docs/testing.md` with the exact production-path and injected-failure
  evidence boundary.

## Non-goals

- Do not add a production fault-injection environment variable, config option,
  alternate syscall mode, dependency, or test-only branch reachable from a
  deployed daemon.
- Do not remove the explicitly weaker fallback used when an operator disables
  the hardened posture on an older kernel.
- Do not change the Linux 6.5 assertion, effective-GID policy, socket mode,
  local-user authorization model, wrapper selection, audit/wire error, runtime
  API, or privileged binary chain.
- Do not weaken, skip, or make conditional any hardened VM assertion merely to
  support the fallback path.
- Do not absorb F-029 bus-liveness evidence, F-030 Nix assertion matrices, F-032
  unsafe rationale, U-002 detailed UAPI review, or any Opportunity.
- Do not claim the tests prove the Linux kernel, eliminate all PID races outside
  this path, or constitute a security audit.

## Acceptance criteria

- The live production resolver and the injected resolver share one decision
  implementation; tests cannot pass while production silently takes a different
  fallback/rejection branch.
- `peer_pidfd` maps exactly `ENOPROTOOPT` to unsupported and preserves every
  other errno/error, including malformed descriptor payloads, as failure.
- Only the unsupported result reads `/proc/<SO_PEERCRED pid>/status` and can
  return `PeerCredFallback`.
- A pidfd acquisition error, missing/unparseable pidfd PID, failed pidfd-target
  status read, malformed status, or credential mismatch returns the existing
  fail-closed rejection and never executes fallback resolution.
- Every successfully acquired pidfd is closed exactly once after its final
  resolution/status use, on both accepted and rejected paths.
- A real socketpair on the supported development/VM kernel resolves through
  `PeerPidfd`; older kernels remain testable only for the explicitly weaker
  fallback contract.
- The hardened VM proves the configured kernel is at least 6.5 and one real
  successful wrapper request logs `auth_path: PeerPidfd`; it rejects any
  `PeerCredFallback` or fallback-warning witness in that scoped record set.
- Existing accepted/mismatched/unresolved peer behavior, generic peer-auth wire
  errors, audit details, and setgid wrapper success/denial behavior remain.
- No dependency, manifest/lock, Nix module, protocol/API, secret-bearing log,
  privileged-chain, F-029+, U-002, or Opportunity change is introduced.

## Security boundary

- Protected asset: brokered resource confidentiality and integrity behind the
  instance setgid wrapper-socket identity gate.
- Attacker: an untrusted local process attempting to exploit PID reuse or a
  silent hardened-path downgrade to impersonate an authorized wrapper peer.
- Boundary direction: local Unix-socket peer → kernel-pinned process identity and
  effective GID → daemon wrapper admission.
- Invariant: in the hardened Linux 6.5+ posture, `SO_PEERPIDFD` is the race-free
  identity path; any failure after choosing it rejects the peer and never falls
  back. The weaker path exists only for explicit non-hardened older-kernel use.

## Validation

- Preserve the red/static baseline: current VM source has no `PeerPidfd` path
  assertion while existing peer-auth unit tests accept either live path and only
  cover an invalid-fd acquisition error.
- `cargo test -p underwrapd peer_auth`
- `cargo test -p underwrapd wrapper_flow::intake`
- `cargo fmt -- --check`
- `cargo clippy --all-targets -p underwrapd -- -D warnings`
- `./crates/integration-tests/run`
- `nix build --no-link .#checks.x86_64-linux.vm-test`
- `nix flake check --all-systems --no-build`
- `nix flake check`
- `git diff --check`
- Static inventory: one production decision flow, complete injected call/close
  traces, exact VM path witness, unchanged module assertion, no dependency/lock,
  generated output, test process, secret-bearing fixture, or fallback weakening.

## Dependencies

- F-027 is complete and automatically accepted at
  `d31a7713ec8fbd9171aae4dfa4e868f0370ee606`; use its supported integration
  runner for cross-component validation.
- F-004/F-005/F-007 and the existing Linux 6.5 hardened assertion remain
  authoritative prerequisites and must not be weakened.
- D-174 authorizes one-ticket-at-a-time continuation subject to all stop,
  cleanup, review, validation, and transition rules.
- F-029 and later findings remain separate.
