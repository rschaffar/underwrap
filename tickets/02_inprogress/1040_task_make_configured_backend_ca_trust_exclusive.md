# Make configured backend CA trust exclusive

Parent: none
Source review: tickets/_review/codebase_2026-07-18.md
Source findings: F-005
Source themes: C-002

## Outcome

When a runtime client instance configures `backend.caFile`/`backend.ca_file`,
`underwrapd` authenticates the backend server only against that operator-selected
CA bundle. Reqwest's bundled public WebPKI roots are disabled for that client, so
a publicly trusted certificate for the configured hostname cannot substitute for
the selected private-CA server.

When no CA file is configured, the existing bundled-WebPKI behavior remains.
Backend URL admission, hostname verification, runtime client certificate
identity, and all runtime registration, resource, policy, and writeback behavior
remain unchanged.

## Scope

- Make the production reqwest client construction select one server trust mode:
  bundled WebPKI roots when `ca_file` is absent, or the configured CA bundle with
  built-in roots disabled when `ca_file` is present.
- Preserve the F-004 HTTPS-only URL and redirect policy and the current ordering
  in which URL admission precedes credential/CA reads and worker creation.
- Add focused policy coverage that observes the root operations applied by the
  same helper/path used to construct the production reqwest client: `None` keeps
  built-in roots, while `Some` disables them before adding the configured roots.
  The regression must fail against the current additive behavior and when a
  focused fault injection removes, bypasses, or flips the production-applied
  built-in-root setting; a detached enum/branch test is insufficient.
- Add supplemental deterministic loopback TLS evidence with two independent
  private server authorities for the same hostname/IP. Through the production
  backend-client construction, the selected CA must authenticate successfully
  and the unselected authority must fail during server authentication before
  receiving an HTTP runtime request. Treat this as selected-root and hostname
  evidence, not by itself as proof that public built-in roots are disabled: an
  arbitrary second private CA was already untrusted before this fix. Every
  listener must have a deadline and be stopped/joined on success and rejection.
- Add one isolated checked-in test-PKI set under
  `crates/integration-tests/testdata/pki/`: selected and unselected CA
  certificates; matching selected/unselected server certificate/key pairs; and
  a wrong-host server certificate/key pair signed by the selected test CA. Use
  the exact fixture stems named in Validation, follow existing test-fixture
  conventions, and ensure the files cross Cargo and Nix/Crane source filters.
  Do not add a dependency, commit a CA signing key, or add a permissive verifier.
- Keep existing real custom-CA mTLS integration and deployed NixOS VM traffic as
  positive evidence, including representative registration, resource read,
  policy, and writeback behavior.
- Update current-state backend, configuration, security, remote-deployment,
  testing, Nix option, and generated-daemon-example wording where relevant to
  state that a configured CA bundle replaces rather than augments bundled
  WebPKI roots.

## Non-goals

- Do not change F-004 URL parsing, HTTPS-only redirects, base-path behavior, or
  proxy-topology semantics, and do not introduce plaintext or insecure modes.
- Do not change backend TLS listeners, operator-browser trust, runtime client
  certificate generation, fingerprint principal resolution, enrollment, runtime
  DTOs/endpoints, policy, resources, or writeback semantics.
- Do not add certificate pinning, TOFU, native-system-root selection, trust-store
  discovery, proxy identity, forwarded certificates, CRL/OCSP/revocation policy,
  CA rotation protocol, or automatic CA/credential reload. Credential reload and
  installation remain F-018 and F-019.
- Do not redesign CA-bundle parsing, hostname/SAN validation, certificate
  lifetime/EKU rules, or general TLS configuration beyond making the existing
  configured-versus-bundled root choice exclusive.
- Do not add a dependency, modify production target configuration, touch the
  backend data model/API, or alter the `underwrap-exec → underwrap-ns`
  privileged chain.
- Do not absorb F-006–F-011, F-017–F-019, F-031's broader CA/path option-shape
  validation, any other review finding, or any Opportunity.

## Acceptance criteria

- With `backend.ca_file = Some(...)`, production client construction disables
  reqwest built-in roots and adds the configured CA certificate bundle. A server
  certificate that chains only to bundled/default WebPKI roots is not accepted
  in this mode.
- With `backend.ca_file = None`, production client construction retains the
  current bundled-WebPKI root behavior; this branch is not accidentally changed
  into an empty trust store.
- Exact focused policy regressions named
  `backend_ca_policy_configured_mode_disables_built_ins` and
  `backend_ca_policy_default_mode_keeps_built_ins` fail if the `Some` and `None`
  built-in-root operations applied to production construction are reversed,
  removed, bypassed, or made additive. They observe the same applied helper/path,
  not only source text or a detached policy decision. Controller fault injection
  that restores additive behavior must make this focused evidence fail.
- Supplemental loopback regressions named
  `backend_ca_selected_authority_is_accepted` and
  `backend_ca_unselected_authority_is_rejected_before_http` use normal
  certificate and hostname verification through the production backend client.
  The configured authority succeeds; the unselected private authority is
  rejected before any HTTP runtime request is observed. Each listener is
  deadline-bounded and stopped/joined in both success and rejection paths.
- `backend_ca_wrong_host_is_rejected_before_http` proves that a server chain under
  the selected CA still fails when its SAN does not match the configured backend
  URL, before any HTTP request is observed. No IP/hostname bypass or custom
  verifier is used.
- The focused TLS tests use no `danger_accept_invalid_*`, custom accept-all
  verifier, external network service, or public Internet dependency. The two
  local private authorities supplement, but do not substitute for, the focused
  built-in-root policy regression.
- Existing repository-PKI HTTPS-to-HTTP downgrade rejection and real custom-CA
  mTLS integration remain green. The focused root-selection fixture does not
  replace real mTLS evidence: the integration suite still proves that the
  production runtime client presents its configured client identity to the real
  backend and completes authenticated runtime calls. The deployed VM's
  module-generated private-CA instances independently register and complete
  representative resource, policy, and writeback traffic.
- Operator documentation and Nix option/example text consistently describe two
  exclusive modes: configured CA bundle only, or bundled WebPKI roots when the
  CA option is absent. They do not claim native-system roots, proxy identity, or
  changed client-certificate semantics.
- The eight named compile-time TLS fixtures are present in the consuming
  integration-test Cargo package inventory and Crane's filtered source. They
  remain under the already retained
  `crates/integration-tests/testdata/pki/` boundary; no broader source-filter
  predicate or repository-wide `testdata/` admission is added. A clean Nix
  workspace build compiles every fixture reference.
- Errors, tests, fixtures, process output, and logs expose no real certificate
  private key, client credential, resource blob, or brokered secret. Any test
  key is clearly isolated fixture material and is not emitted at runtime.
- Focused daemon, integration, full workspace, clippy, rustfmt, Cargo/Nix source
  filtering, non-coverage flake, and deployed VM checks pass. A pre-existing
  toolchain failure may be reported only with exact clean-baseline evidence; it
  must not be hidden or fixed by widening this ticket.

## Security boundary

- Protected asset: backend identity, runtime resource confidentiality and
  authenticity, permission decisions, and submitted writebacks.
- Attacker: a network peer that can present a publicly trusted certificate for
  the configured backend hostname but cannot chain to the operator-selected
  private CA.
- Boundary direction: server → daemon authentication gates the subsequent
  bidirectional runtime channel.
- Invariant: when an operator configures a backend CA, that selected trust set is
  exclusive; a public WebPKI chain cannot authenticate the backend in parallel.

## Validation

- `cargo test -p underwrapd backend_client::tests::backend_ca_policy_ -- --nocapture`
  (exactly the two named policy tests above)
- `cargo test -p integration-tests backend_ca_ -- --nocapture` (exactly the three
  named loopback TLS tests above)
- `cargo test -p underwrapd https_only_redirect`
- `cargo test -p underwrapd`
- `cargo test -p integration-tests`
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `nix build .#checks.x86_64-linux.workspace-test`
- `nix build .#checks.x86_64-linux.examples-nix-parse`
- `nix build .#checks.x86_64-linux.vm-test`
- `nix flake check`
- `diff -u <(printf '%s\n' testdata/pki/ca-selected-exclusive.pem testdata/pki/ca-unselected.pem testdata/pki/server-selected-exclusive.pem testdata/pki/server-selected-exclusive.key testdata/pki/server-unselected-ca.pem testdata/pki/server-unselected-ca.key testdata/pki/server-wrong-host.pem testdata/pki/server-wrong-host.key | LC_ALL=C sort) <(cargo package -p integration-tests --allow-dirty --list | grep -E '^testdata/pki/(ca-selected-exclusive|ca-unselected|server-selected-exclusive|server-unselected-ca|server-wrong-host)\.(pem|key)$' | LC_ALL=C sort)`
- `rg -n 'crates/integration-tests/testdata/' flake.nix`
- `git status --short`
- `git diff --name-only`
- `git diff --check`
- `! git diff -U0 -- . ':(exclude)crates/integration-tests/testdata/pki/**' | rg '^\+.*-----BEGIN (CERTIFICATE|[^ ]*PRIVATE KEY)-----'`

## Dependencies

- `tickets/03_done/1030_task_reject_plaintext_runtime_backend_urls.md` —
  satisfied; HTTPS admission and downgrade rejection are enforced before server
  trust is narrowed.
