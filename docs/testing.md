# Testing

underwrap uses four complementary validation layers:

1. Rust unit and integration tests;
2. SPA unit/build/browser tests;
3. workspace lint, formatting, audit, dependency-policy, and coverage checks;
4. a NixOS VM test of the deployed privilege and namespace model.

No layer is a security proof. Together they detect regressions in code paths,
wire contracts, packaging, and the real NixOS wrapper chain.

## Fast local loop

For one Rust crate:

```bash
cargo test -p <crate>
```

For cross-component daemon/backend behavior:

```bash
cargo test -p integration-tests
```

For the SPA:

```bash
cd web
npm ci                  # first run or after lockfile changes
npm test
npm run build
```

Run formatting and lint before committing Rust changes:

```bash
cargo fmt -- --check
cargo clippy --all-targets --workspace -- -D warnings
```

## Complete repository checks

The flake declares these checks:

```bash
nix build .#checks.x86_64-linux.workspace-test
nix build .#checks.x86_64-linux.workspace-nextest
nix build .#checks.x86_64-linux.workspace-clippy
nix build .#checks.x86_64-linux.workspace-fmt
nix build .#checks.x86_64-linux.workspace-audit
nix build .#checks.x86_64-linux.workspace-deny
nix build .#checks.x86_64-linux.workspace-llvm-cov
nix build .#checks.x86_64-linux.examples-nix-parse
nix build .#checks.x86_64-linux.frontend-backend-url-eval
nix build .#checks.x86_64-linux.frontend-swap-posture-eval
nix build .#checks.x86_64-linux.vm-test
```

Run all flake checks for the current system with:

```bash
nix flake check
```

SPA unit tests are not a separate flake check, so run them explicitly:

```bash
cd web && npm test && npm run build
```

## Rust coverage

Workspace tests cover:

- config parsing and validation;
- IPC framing and SCM_RIGHTS transfer;
- runtime/admin DTO serialization;
- permission mode evaluation;
- canonical resource blob handling;
- plain and encrypted storage;
- daemon peer authentication and wrapper intake;
- backend runtime policy and writeback validation;
- target environment, capability, UID, idmap, mount, and writeback helpers;
- Secret Service data model and D-Bus handler behavior;
- private libsecret framing, strict canonical/semantic validation, exact
  generation ACK/REJECT, fallback, terminal EOF/EAGAIN, and near-cap
  backpressure;
- real daemon/backend integration flows.

Some integration tests skip when the host cannot supply the required Linux
capabilities. Focused `underwrap-ns` tests cover complete-plan zero-side-effect
rejection, exact gate parsing, CLOEXEC/opposite-end ownership, returned-exec
failure reporting, helper/target fd non-retention, parent death before/after
arming and after credential-clear rearming, inherited SIGCHLD restoration,
exact nonblocking target-status framing, wait failure, the real gate × status ×
relay dispatch seam, and libsecret publication framing/selection. The gate,
status, and publication channels are independent evidence; none authorizes a
lifecycle by itself. The VM independently covers the deployed privileged chain,
relay/PID-2 topology, supervisor/relay termination, complete namespace teardown,
status/writeback preservation, old same-UID pathname irrelevance, legitimate
mutation persistence, near-cap publication persisted through Checkin and
subsequent D-Bus readback, exact-2-MiB files round trips, rejected oversize
admin/files mutations with byte-identical prior storage, host-only
receiver/secretsd-only publisher ownership, and plaintext/exact-base64 log
hygiene.
Daemon transport tests cover absolute HTTPS URL admission, base-path preservation,
and HTTPS-to-HTTP redirect rejection through the production client. Applied
root-policy unit tests record that the production client disables bundled
WebPKI roots before adding a configured CA, while omission explicitly retains
the bundled roots. Supplemental loopback probes use two independent private
server authorities and a wrong-host certificate to prove selected-chain and
hostname behavior through `BackendClient::open` and register; those probes do
not require server-side client authentication, and rejection of an arbitrary
second private CA alone is not evidence that public roots are excluded. The
separate real-backend integration suite retains mTLS client-certificate,
fingerprint-principal, registration, resource, policy, and writeback evidence,
and the NixOS VM exercises representative deployed private-CA traffic.

The `frontend-backend-url-eval` check proves module-level HTTP rejection and
exact HTTPS URL generation; the VM independently proves a handcrafted plaintext
configuration fails before credentials, socket readiness, or network traffic.

The `frontend-swap-posture-eval` check evaluates the production module across
no-swap, ordinary encrypted/plain swap, high-level and typed zram-generator,
mixed-failure, mounted-filesystem, inert, INI name-normalization and multiline
section/directive-injection, and explicitly weaker-posture rows. It proves exact
module-local swap-assertion presence, absence, and truth, checks the complete set
of false underwrap assertion messages, and compares the merged false-assertion
boolean count so an unrelated failure cannot pass unnoticed without forcing
unrelated lazy assertion messages. This covers declared NixOS configuration,
not arbitrary external `/etc` drop-ins or live runtime device state.

## OpenAPI drift

The backend admin API is generated from `utoipa` annotations. A workspace test
compares the generated document with:

```text
crates/underwrap-backend/openapi.json
```

When handlers or DTOs change, run:

```bash
cargo test --workspace
cd web && npm run generate:api && npm run build
```

Review and commit the OpenAPI change with the implementation. The generated
TypeScript schema under `web/src/api/schema.ts` is local build output and is not
committed.

## SPA browser tests

Playwright exercises protected routing, authentication, CSRF, stores, secrets,
permissions, client instances, and user workflows.

Run it inside the Nix development shell so Playwright uses the NixOS-compatible
browser:

```bash
nix develop
cd web
npm run test:e2e
```

For an interactive browser runner:

```bash
npm run test:e2e:ui
```

## NixOS VM test

The VM test boots a real NixOS system with the module, backend processes,
frontend instances, wrappers, unprivileged callers, and target programs.

### Packaging and service layout

It checks:

- users, groups, state/runtime directories, and systemd units;
- setgid and capability wrappers;
- socket path, owner, group, and mode;
- generated daemon and entry-stage config;
- local and remote-style runtime client enrollment.

### Wrapper identity and policy

It checks:

- effective-GID wrapper-socket peer authentication;
- spoofing and direct-socket rejection;
- rejection of a complete forged fd-3/fd-20 direct `underwrap-ns` handoff,
  followed by a successful daemon-created handoff;
- daemon validation of wrapper selectors;
- daemon-chosen real/debug targets;
- locked, unlocked, expiring, unsafe-debug, missing, and read-only permission
  behavior;
- per-wrapper/resource registration and usage timestamps.

### Target isolation

It checks that each `(instance, wrapper)` has a distinct dedicated target UID,
including wrappers that share a target binary and wrappers across instances.
It verifies real and debug variants use the intended identity and execute with
zero capabilities.

Against the target and a descendant, caller-owned peers and ancestors are denied
access to:

- `/proc/<pid>/{environ,maps,smaps,auxv,mem}`;
- descriptor and executable links under `/proc/<pid>`;
- `ptrace(PTRACE_ATTACH)`;
- `process_vm_readv`.

It also confirms that world-readable process metadata remains secret-free and
that root retains its documented visibility.

### Namespace and projection behavior

It checks:

- fresh PID and `/proc` views;
- complete files-projection prevalidation, routing, staging, and writeback;
- later-unroutable projection and returned-exec failures through the real
  setgid/capability chain, including target non-execution, canonical blob byte
  equality, and exact absence of terminal daemon audit verbs;
- private D-Bus and Secret Service operation;
- target-owned runtime directories;
- idmapped caller access paths and resulting file ownership;
- host-namespace inability to see private projection mounts or sockets;
- read-only mutation discard and warning/audit behavior;
- core-dump prevention and descriptor hygiene;
- private exec-gate authorization and retained post-exec persistence/status for
  a file-writing target that exits nonzero or dies by signal;
- a resource env value above 64 KiB through the deployed authenticated bulk
  Checkout path, and exact-2-MiB files-resource Checkout/Checkin, oversize
  finalization suppression, stable admin size rejection, and byte-identical
  retained storage.

### Secret hygiene

It checks successful delivery while asserting that configured secret values do
not appear in service logs.

## Validation by change type

| Change | Required validation |
| --- | --- |
| Pure Rust logic in one crate | crate tests, fmt, clippy |
| Runtime DTO or policy | protocol + backend + daemon tests, integration tests |
| Admin handler or DTO | backend tests, OpenAPI drift, SPA test/build |
| SPA behavior | Vitest, build; Playwright for user flows |
| Storage or encryption | store/backend tests, integration tests |
| NixOS module | focused evaluator checks, examples, workspace Nix checks, VM test |
| setgid, peer auth, UID, namespace, capabilities, mounts, fd lifecycle | integration tests and VM test |
| Documentation/examples only | link/path checks, example parse/evaluation tests, relevant generated-schema checks |

## What tests do not establish

Automated tests do not establish:

- absence of kernel or dependency vulnerabilities;
- correctness on every Linux filesystem and desktop environment;
- safety of a broad or malicious target;
- protection after root or backend compromise;
- formal noninterference or a complete security proof.

Security-sensitive changes still require human review against
[Security](security.md).

## Related documentation

- [Architecture](architecture.md)
- [Security](security.md)
- [`AGENTS.md`](../AGENTS.md)
