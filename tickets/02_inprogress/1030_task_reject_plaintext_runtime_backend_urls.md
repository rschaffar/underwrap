# Reject plaintext runtime backend URLs

Parent: none
Source review: tickets/_review/codebase_2026-07-18.md
Source findings: F-004
Source themes: C-002

## Outcome

Every runtime client instance communicates with `underwrap-backend` only over
HTTPS with mTLS. The NixOS frontend configuration rejects a plaintext backend
URL during evaluation, and `underwrapd` independently rejects a non-HTTPS or
invalid backend URL before it opens a runtime client or accepts wrapper
requests. The reqwest client itself is HTTPS-only, so a redirect or future call
path cannot silently downgrade an admitted HTTPS configuration to plaintext.

Valid HTTPS deployments keep their current runtime registration, resource,
policy, and writeback behavior. Plaintext transport is invalid first-release
configuration rather than an alternate posture or compatibility mode.

The enforced URL is the endpoint that `underwrapd` dials. A URL naming an HTTPS
reverse-proxy frontend passes this transport-admission check. That fact alone
does not make ordinary TLS-terminating HTTP proxying a supported runtime
topology: the backend currently derives the runtime client instance from the
peer certificate on its own rustls connection. Direct backend TLS and Layer-4
TLS passthrough preserve that end-to-end mTLS principal; this ticket does not add
a trusted forwarded-certificate or proxy-identity protocol.

## Scope

- Parse the daemon's configured backend base URL as an absolute URL, require the
  `https` scheme and a host, and return a clear fail-closed configuration error
  for empty, malformed, hostless, or non-HTTPS values.
- Apply that validation in `underwrapd` startup before credential reads, socket
  binding, backend worker creation, or network traffic. Also enforce it at the
  backend-client construction boundary so an internal caller cannot bypass the
  startup check.
- Configure reqwest with its HTTPS-only policy. Keep all runtime registration,
  invocation-open, and finalize requests under that policy, including redirect
  handling.
- Make `services.underwrap.instances.<name>.backend.url` require the documented
  `https://` form during NixOS module evaluation, with an instance-specific
  error that names the invalid option but does not expose credentials.
- Add durable negative evidence for both configuration authorities: daemon tests
  for plaintext/malformed URLs and error ordering, plus a focused Nix evaluation
  check that proves an `http://` instance is rejected while an equivalent
  `https://` instance is admitted. Add a backend-client regression using the
  production reqwest construction which receives an HTTPS-to-HTTP redirect from
  a trusted test endpoint and proves the plaintext trap receives no connection;
  reuse repository test PKI, keep normal certificate verification, and add no
  permissive verifier.
- Extend the deployed VM scenario to prove a handcrafted plaintext daemon
  configuration cannot initialize or contact a plaintext endpoint, while the
  normal generated HTTPS/mTLS runtime path still performs representative
  registration, resource reads, policy decisions, and writeback.
- Update current-state configuration, remote-deployment, security, and testing
  documentation to state that HTTPS is mandatory and enforced independently by
  Nix evaluation, daemon startup, and the request client. Distinguish the URL
  admission rule from reverse-proxy topology: an HTTPS proxy address is valid,
  while the existing runtime principal still requires the client certificate to
  reach the backend TLS connection unless a separately designed mechanism says
  otherwise.

## Non-goals

- Do not absorb F-005. This ticket does not change whether a configured private
  CA replaces or augments reqwest's bundled WebPKI roots, nor add the later
  two-CA exclusivity test.
- Do not add an insecure override or let `underwrapd` dial plaintext HTTP, even
  for a loopback address or a TLS proxy's upstream hop. Do not add opportunistic
  TLS, SSH tunnel transport, Unix-socket backend transport, or automatic URL
  rewriting from `http` to `https`.
- Do not change the backend HTTPS listener, admin-browser development proxy,
  runtime API endpoints/DTOs, mTLS fingerprint principal model, policy, resource
  format, or writeback semantics. Do not add trusted proxy headers, forwarded
  client-certificate parsing, a proxy service identity, or generic reverse-proxy
  support.
- Do not turn this into general URL-product redesign. Beyond parsing an absolute
  HTTPS URL with a host, do not remove supported HTTPS base paths, define a new
  query/fragment/userinfo policy, add discovery, or add proxy configuration.
- Do not absorb F-010's zram hardening, F-017's runtime request concurrency,
  F-018's credential-reload documentation, F-019's credential installation,
  F-026/F-057 aggregate-check work, or any Opportunity.
- Do not add a dependency, change production target configuration, alter
  `underwrapd`'s runtime protocol, modify the production backend data model, or
  touch the `underwrap-exec → underwrap-ns` privileged chain.

## Acceptance criteria

- `underwrapd` accepts representative absolute HTTPS backend URLs with a host,
  including a URL with a non-root base path and trailing slash, and rejects
  `http://`, other schemes, relative strings, malformed URLs, and hostless HTTPS
  values with a deterministic configuration error that identifies `backend.url`
  and the HTTPS requirement. Runtime endpoint construction preserves the
  existing base-path behavior: for example,
  `https://backend.example.internal/base/` addresses registration beneath
  `/base/v1/wrappers` rather than discarding `/base`.
- Rejection happens before the daemon reads client certificate/key files,
  creates its backend worker, connects to any endpoint, binds the wrapper socket,
  or reports readiness. A missing or malformed credential cannot mask an
  inadmissible plaintext URL.
- `BackendClient::open` independently rejects a non-HTTPS URL before reading
  CA/client certificate/key files or starting its worker, and its reqwest builder
  uses HTTPS-only mode. A focused production-client test proves registration,
  open, or finalize cannot follow a trusted HTTPS endpoint's redirect to a
  plaintext trap; the trap observes zero connections. The test does not disable
  certificate verification or alter private-CA root selection.
- Nix evaluation of an enabled instance with
  `backend.url = "http://backend.example.invalid"` fails on an assertion or
  option check naming
  `services.underwrap.instances.<name>.backend.url`. The same focused evaluator
  admits `https://backend.example.invalid`; the generated daemon configuration
  retains that HTTPS URL exactly.
- A committed focused Nix check exercises both the negative and positive module
  cases rather than relying only on an ad hoc validation command. Existing
  complete Nix examples continue to evaluate.
- The deployed VM gives a standalone `underwrapd` a handcrafted plaintext URL,
  observes startup failure before wrapper-socket readiness and before any
  plaintext trap receives a connection, then proves the module-generated HTTPS
  instances still register and complete representative authenticated resource,
  policy, and writeback traffic.
- Existing real custom-CA HTTPS/mTLS integration tests remain green. Tests do not
  weaken TLS verification, install a permissive certificate verifier, or claim
  F-005's private-CA exclusivity.
- Published documentation states the enforced HTTPS-only runtime posture in the
  present tense and offers no plaintext workaround. It says the check applies to
  the endpoint `underwrapd` dials, so an `https://` nginx address is admitted,
  while ordinary HTTP proxying after TLS termination is not newly supported and
  cannot preserve the backend's peer-certificate runtime principal without a
  separate approved design. Examples remain valid HTTPS configurations.
- Error messages, tests, process output, and logs contain no client private key,
  certificate body, resource blob, or brokered secret. No secret enters argv or
  environment during the VM negative test.
- Focused daemon, Nix evaluation, integration, full workspace, clippy, rustfmt,
  examples, and deployed VM checks pass.

## Security boundary

- Protected asset: runtime resource values and policy confidentiality and
  authenticity, plus integrity of permission decisions and submitted
  writebacks.
- Attacker: a network observer or active on-path peer between `underwrapd` and
  the backend. An operator typo or plaintext reverse-proxy configuration is the
  trigger that would otherwise expose the channel; no compromised trusted host
  is required.
- Boundary direction: backend → daemon for resource blobs and policy decisions,
  and daemon → backend for runtime registration and writeback. A client identity
  attached to reqwest proves nothing when no TLS handshake occurs.
- Invariant: the documented runtime channel is authenticated HTTPS with mTLS.
  Nix evaluation, daemon admission, and the HTTP client all fail closed rather
  than allowing plaintext transport or downgrade.

## Validation

- `cargo test -p underwrapd backend_url`
- `cargo test -p underwrapd https_only_redirect`
- `cargo test -p underwrapd`
- `cargo test -p integration-tests`
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `nix build .#checks.x86_64-linux.frontend-backend-url-eval`
- `nix build .#checks.x86_64-linux.examples-nix-parse`
- `nix build .#checks.x86_64-linux.vm-test`
- `nix flake check`
- focused VM assertions for plaintext-trap zero accepts, absent wrapper
  socket/readiness, URL-error ordering over unusable credential paths, and
  secret-free diagnostics
- `git diff --check`

## Dependencies

- `tickets/03_done/1010_task_require_offline_first_operator_bootstrap.md` —
  satisfied; deployment authority is no longer establishable by the first
  network caller.
- `tickets/03_done/1020_task_retain_recoverable_rotation_generations.md` —
  satisfied; response-loss recovery is established before the remaining runtime
  transport boundary is tightened.
