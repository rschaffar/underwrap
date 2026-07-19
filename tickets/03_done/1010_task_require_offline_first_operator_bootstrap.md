# Require deployment-authorized offline first-operator bootstrap

Parent: none
Source review: tickets/_review/codebase_2026-07-18.md
Source findings: F-002
Source themes: C-002

## Outcome

No network client can create the first backend administrator. A deployment
operator establishes exactly one initial account through a quiesced offline
command executed with backend-host filesystem authority, with the password read
from stdin. The running SPA reports that setup is required but never accepts or
submits first-user credentials; after offline provisioning, the operator signs
in through the ordinary login flow.

This replaces race-to-win public bootstrap with deployment-side authority while
preserving the intended first operator, password, sentinel, session-login, and
subsequent administration semantics.

## Scope

- Add a distinct `underwrap-backend admin bootstrap-user --username <name>`
  operation. It uses the configured `users_dir` and `bootstrap_file`, reads one
  password from stdin, applies the existing first-user username/password rules,
  and runs without a listener while the backend service is stopped.
- Define fresh bootstrap state as an absent sentinel plus an empty, valid
  operator collection. Existing stores, resources, policy, or runtime client
  records do not participate and remain untouched.
- Serialize independent bootstrap command processes with a stable filesystem
  lock derived from the configured authentication-state paths. Hold it across
  the authoritative re-check, password hashing, user persistence, and sentinel
  persistence; use existing dependencies and document every syscall/ownership
  invariant.
- Preserve user-before-sentinel ordering. Successful completion requires both
  artifacts; user-write failure leaves no sentinel, while sentinel-write failure
  reports failure but preserves the one valid login-capable user rather than
  attempting a destructive rollback.
- Remove the public first-user creation handler, request/response DTOs, route,
  immediate session issuance, typed SPA call, and production state that becomes
  dead with that route. Retain the public read-only bootstrap-status endpoint.
- Turn `/bootstrap` into an instruction-only setup-required page. Update
  generated OpenAPI, current-state documentation, examples, comments, and the
  normal login handoff to describe the offline authority boundary.
- Convert Rust integration, real-backend browser, and NixOS VM fixtures to
  provision the first operator before the listener starts and then authenticate
  over the ordinary login endpoint. Add hostile-network, cross-process,
  persistence-failure, service-identity, and DAC regressions.

## Non-goals

- Do not add a bootstrap bearer token, verifier/token file, systemd credential,
  local bootstrap listener/socket, new configuration credential, or dependency.
- Do not change `recover-user` append/refuse-on-conflict behavior, its unchanged-
  sentinel semantics, or publish the broader lockout-recovery procedure; those
  remain F-034 scope.
- Do not correct unrelated OpenAPI authentication/status defects from F-016.
  Removing this route and its dead schemas from generated artifacts is only
  contract synchronization for F-002.
- Do not redesign public login Argon2 concurrency, rate limiting, or limiter
  memory (Opportunity O-006), OpenAPI generation (O-023), or process/port fixture
  cleanup (O-024).
- Do not add roles, per-store operator grants, session redesign, password-policy
  changes, or changes to stores, resources, policy, runtime client transport,
  F-003/F-004, the NixOS frontend module, or the
  `underwrap-exec → underwrap-ns` chain.

## Acceptance criteria

- The live admin router and generated OpenAPI contain no operation that creates
  the first operator. A valid-looking unauthenticated
  `POST /v1/admin/auth/bootstrap`, with or without an allowed `Origin`, receives
  the router's normal not-found/method-not-allowed response and creates neither
  a user record nor the sentinel.
- `GET /v1/admin/auth/bootstrap/status` remains read-only and returns setup
  required only when the sentinel is absent and the valid operator collection is
  empty. Sentinel/user read errors, malformed user state, an existing user, or a
  set sentinel fail closed to not-required.
- `underwrap-backend --config <path> admin bootstrap-user --username <name>`
  works without a running listener, reads the password only from stdin, uses the
  configured authentication-state paths, normalizes/rejects the username, and
  enforces the existing minimum-password policy.
- Bootstrap freshness is limited to the operator collection and sentinel.
  Pre-existing stores, resources, policy, and runtime client records neither
  block setup nor change byte-for-byte.
- Empty/short password, empty username, malformed or unreadable state, an
  existing user, a set or unreadable sentinel, and a repeated invocation fail
  without creating, overwriting, or mutating another operator or backend asset.
- Two concurrent real command processes are cross-process serialized: exactly
  one succeeds, the other refuses, and one user plus one sentinel remain. A
  crashed process cannot leave a permanently held lock.
- The user record is committed before the sentinel. A forced user-save failure
  leaves no user and no sentinel. A forced sentinel-save failure leaves one
  valid login-capable user and no sentinel, returns a precise non-secret error,
  performs no rollback/deletion, and causes public status to remain fail closed.
  Full success leaves one verifiable user and the sentinel.
- A second sequential invocation leaves the original user file, password hash,
  timestamps, sentinel, and unrelated state unchanged. `recover-user` retains
  its existing behavior and tests.
- No raw password, password hash, session value, bootstrap capability, or other
  credential enters argv, environment, configuration, logs, errors, test
  diagnostics, or a new persistence path. Audit/log output contains only safe
  operation/result identifiers and non-secret identity/path metadata.
- An uninitialized SPA routes to an actionable instruction-only `/bootstrap`
  page with no username/password fields, create button, or credential POST.
  After offline provisioning, normal login issues the session/CSRF state and
  reaches the protected SPA.
- In-process and real-binary Rust integration fixtures, Playwright fixtures, and
  the VM provision before listener startup and log in afterward. Real HTTPS and
  deployed VM regressions prove network bootstrap cannot claim a fresh backend;
  the VM also proves the backend service identity can provision while an
  ordinary user cannot write the service-owned authentication state.
- OpenAPI/generated client artifacts and present-tense bootstrap documentation
  describe only this first-release flow. F-016, F-034, O-006, O-023, and O-024
  remain unresolved and unclaimed.

## Security boundary

- Protected asset: first authority over every backend administrative asset,
  including operators, stores, resources, policy, runtime client instances, and
  unlocked key operations; also the initial operator password and hash.
- Attacker: an unauthenticated remote client that can reach a newly started
  backend before the deployment operator.
- Boundary direction: untrusted network → public admin router must never create
  authority; trusted backend-host root/service identity → quiesced offline
  command may create the initial operator through service-owned state.
- Invariant: `AGENTS.md` requires security mechanisms to fail closed, and
  `docs/security.md` places host root, the backend service, and its storage under
  the trusted computing base. Race exclusivity and optional browser Origin are
  not authorization; only deployment-side filesystem authority may establish
  the first administrator.

## Validation

- `cargo test -p underwrap-backend bootstrap`
- `cargo test -p underwrap-backend`
- `cargo test -p integration-tests network_bootstrap_cannot_create_operator`
- `cargo test -p integration-tests`
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cd web && npm test && npm run build`
- `nix develop --command bash -lc 'cd web && npm run test:e2e'`
- `nix build .#checks.x86_64-linux.vm-test`

## Dependencies

- None.
