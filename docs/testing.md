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
- canonical resource blob handling, including deterministic identifier-only
  enumeration of env, files, libsecret aliases, empty/nonempty collections, and
  items;
- plain and encrypted storage, including active resource-key absence and
  repeated false deletion after removing a typed-empty blob;
- daemon peer authentication and wrapper intake;
- daemon runtime-client admission and progress: the exact production 32-call/
  30-second policy, independent direct async requests, timeout permit release,
  fail-fast saturation without request start, and later admission;
- backend runtime registration validation before storage, including exact
  within-request duplicate rejection, name-error precedence, valid near misses,
  checked internal insertion failures, and separate-call idempotence;
- backend runtime policy and writeback validation;
- six registry failure-atomicity unit matrices—store create/update/delete and
  runtime client instance create/update/delete—using real directory
  rename/regular-file blockers. They pin the exact 500 codes, unchanged live
  and freshly loaded records after helper failure, absence of failed success
  audit/cascade effects, successful retry publication, and store backend/state/
  rotation-lock or complete client-record identity where applicable;
- resource deletion blocker aggregation, malformed-blob fail-before behavior,
  blob-delete-before-catalog ordering, storage-failure state re-read, and
  absence of reconstruction or false success;
- centralized selected-store admin reference checks: every explicit flat route
  and all eight libsecret operations reject a missing parent before blob access;
  permission upsert checks client/store, resource, and exact pair in order before
  policy access; ordinary pair create uses the canonical value-free resource
  404; rejection snapshots and operation traces prove no effect while valid,
  disabled-client, missing-child, and idempotent-delete behavior remains intact;
- target environment, capability, UID, idmap, mount, and writeback helpers;
- Secret Service data model and production D-Bus handler behavior, including the
  collection/item/alias mutation rejection matrix with structural live-store
  equality, failed-create object-registration absence, ACKed create and
  replacement paths, write-lock retention through exact ACK, wrong-generation
  and EOF preservation, and a rejected generation followed by an unrelated
  accepted generation;
- private libsecret framing, strict canonical/semantic validation, exact
  generation ACK/REJECT, fallback, terminal EOF/EAGAIN, and near-cap
  backpressure;
- real daemon/backend integration flows.

The handler tests enforce the production ordering: retain the write lock, clone
the accepted store once, mutate the candidate, canonicalize and publish it,
observe the exact matching ACK, and only then commit. Preparation, publication,
rejection, EOF, response mismatch, and transport failures preserve accepted live
memory. Tests directly cover the complete-store zeroization helper and handler
routes. Source/static inventory verifies that discarded candidate and displaced
secret buffers, including deletion and replacement paths, are zeroized before
drop; the suite does not instrument memory after drop. The lifecycle tests keep
the separate host-side consequence explicit: a completely written ACK can remain
eligible when secretsd or the D-Bus caller misses success, but ACK is neither
caller delivery nor backend durability and a later Checkin can still fail.

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

The keygen identity-installation tests exercise a five-phase precommit failure
matrix: complete temporary-file write, owner application, exact mode application,
file sync, and the persist boundary. Every injected failure must preserve the old
complete identity byte-for-byte with identical UID, GID, and mode and leave no
staging entry. A separate success test requires one complete new identity with
exact metadata, and a post-commit stdout-failure test proves that failed
public-certificate delivery leaves that complete new identity installed. These tests
pin persist as the visibility commit; they do not claim parent-directory fsync,
power-loss durability, rollback, secure erasure, or a frontend/backend
transaction.

Protocol tests pin the sole config field `client_identity_file`; keygen tests
pin the sole explicit CLI flag `--client-identity-out`. Daemon startup and
backend-client tests cover one-file admission, URL-before-file-I/O ordering,
construction from the combined identity, CA policy, redirects, and typed errors.
Integration PKI and daemon/backend-CA tests build and consume one combined
identity while retaining separate certificate/key source fixtures only where a
test independently needs those primitives.

The daemon runtime-client concurrency tests are deterministic TLS fixtures. The
current-thread Tokio test
`independent_runtime_requests_are_not_globally_serialized` synchronizes after the
server has accepted and completely read one invocation-open request, withholds
that response, and proves an unrelated request reaches the server and completes
before release. This detects both a global FIFO worker and a synchronous call
that blocks the single runtime thread.
`accepted_request_timeout_releases_its_permit` injects a short client deadline
for an accepted but unanswered request and proves timeout, permit release, and a
later successful admission.
`saturation_is_fail_fast_without_starting_a_request_and_later_admission_succeeds`
is a deterministic direct-closure test: it injects one slot, holds its request
future, proves the saturated closure is never constructed or started, and then
proves admission succeeds after release.
`production_runtime_request_policy_is_pinned` checks the exact production
constants of 32 calls and 30 seconds without making the test wait for the
production deadline.

The 30-second value is a product request deadline, measured after admission
through complete success/error decoding. Fixture socket, channel, coordination,
and injected short-client deadlines are test cleanup and assertion bounds; they
do not redefine that product contract. The parent process that invokes the test
command remains the hard whole-suite bound. Cancellation-unsafe blocking futures
are not placed under an in-runtime timeout merely to bound the suite.

Daemon transport tests cover absolute HTTPS URL admission, base-path preservation,
and asynchronous HTTPS-to-HTTP redirect rejection through the production
client, including zero connections to the plaintext trap. Applied root-policy
unit tests record that the production client disables bundled
WebPKI roots before adding a configured CA, while omission explicitly retains
the bundled roots. Supplemental loopback probes use two independent private
server authorities and a wrong-host certificate to prove selected-chain and
hostname behavior through `BackendClient::open` and register; those probes do
not require server-side client authentication, and rejection of an arbitrary
second private CA alone is not evidence that public roots are excluded. The
separate real-backend integration suite retains mTLS client-certificate,
fingerprint-principal, registration, resource, policy, and writeback evidence.
Together with the asynchronous integration `backend_ca_*` private-CA/hostname
probes, registration tests, daemon open/store-read flows, and finalize/writeback
and UsageSignal flows, this is aggregate evidence that direct async scheduling
did not change transport, authority, error mapping, or persistence. The NixOS
VM test in `nix/tests/test.py` provides the backend-service restart evidence;
the Rust integration aggregate does not establish credential-file reload by a
running daemon. The aggregate is not evidence of availability once all
32 permits are occupied or while the backend is unavailable.
Its registration regression sends duplicate rows over real client-certificate-
authenticated HTTPS with auto-create on and off against missing and existing
state, verifies the exact structured 400 without a TLS EOF, compares unchanged
resource/pair/policy views, and then reuses the service and client for valid and
separate-call-idempotent registration. The NixOS VM exercises representative
deployed private-CA traffic. The
real authenticated HTTPS admin suite also covers a libsecret-only deletion
blocker, unchanged catalog/blob state while blocked, structured cleanup,
successful deletion, and direct active-key absence. Its admin-reference matrix
exercises representative flat, libsecret, permission, and pair 404 envelopes,
compares unchanged unfiltered secrets, resources, pairs, policy, and runtime
client views, verifies no orphan active key appeared, and then reuses the same
live service and authenticated session for valid content and permission writes.
A separate real authenticated HTTPS/mTLS registry matrix blocks the actual
store/client registry paths while one live service and admin session performs
store update, store delete, and all-field client update. It checks exact 500
codes, unchanged authenticated list and fresh-load state, successful retries,
local-to-tunnel certificate replacement, old-certificate rejection, and
replacement disable/re-enable behavior. The normal path explicitly restores the
directory before state assertions; `Drop` provides only a best-effort fallback.
The daemon/backend stop future is always awaited without an in-test timeout.

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

## OpenAPI semantics and drift

The backend admin API is generated from `utoipa` annotations. Semantic tests in
`crates/underwrap-backend/tests/openapi_export.rs` inspect
`admin_openapi_spec()` and pin:

- traversal of all eight OpenAPI operation keys (`GET`, `PUT`, `POST`, `DELETE`,
  `OPTIONS`, `HEAD`, `PATCH`, and `TRACE`), so a newly introduced method cannot
  escape the contract checks;
- absent-or-empty root security and effective OpenAPI inheritance semantics:
  absent operation security inherits the root, while an explicit empty array
  is public;
- the exact three effectively public operations, session-only protected reads,
  and one session-and-CSRF Security Requirement Object on each of the exact 32
  protected mutations (never two OR-shaped objects);
- the exact `apiKey` definitions for the `__Host-underwrap-session` cookie and
  `X-CSRF-Token` header schemes;
- zero 423 responses and the exact total set of 27 operations carrying any 503
  response, followed by exact `store_locked` descriptions and
  `ServiceErrorResponse` schemas;
- the exact lock, rotate-key, and unlock response sets and every canonical
  lifecycle code in their descriptions.

Live lifecycle handler tests pin statuses and codes actually emitted. The
semantic OpenAPI tests pin generated meaning. The byte-drift test separately
compares the generated bytes with:

```text
crates/underwrap-backend/openapi.json
```

When handler annotations or DTOs change, run:

```bash
cargo test --workspace
cd web && npm run generate:api && npm run build
```

The drift test rewrites `openapi.json` in place when needed; review and commit
that change with the implementation. The generated TypeScript schema under
`web/src/api/schema.ts` is local build output and is not committed.

## SPA secret-text unit tests

Vitest pins the browser's text-only secret conversion boundary independently of
the backend's arbitrary-byte contract. Known UTF-8/base64 vectors include
`✓ à la mode` → `4pyTIMOgIGxhIG1vZGU=`, `päss🔐` → `cMOkc3Pwn5SQ`,
Japanese text, empty text, and a multi-chunk input. Component tests pin exact
Unicode request bodies for flat create/rename and libsecret item creation, plus
exact reveal and clipboard text. Reveal tests distinguish missing and empty
values, malformed base64, and valid base64 for non-UTF-8 bytes (`//4=` for
`0xff 0xfe`); the invalid cases never reach the textarea or clipboard.

Libsecret attribute parser tests cover one `key=value` entry per nonblank line,
first-delimiter splitting, trimmed keys and values, ignored blank lines, CRLF,
empty values, and case-sensitive keys. They also pin line-numbered rejection of
missing delimiters, empty trimmed keys, and repeated exact trimmed keys (with
the original line), plus safe own-property handling for object-like names such
as `__proto__` and `constructor`. Create-item tests prove those field errors
send no request and that valid mixed input sends the exact attribute map while
retaining the Unicode secret bytes. These are browser checks, not backend/API
attribute rejection tests.

Run the focused boundary tests with:

```bash
cd web
npm test -- src/lib/utf8-base64.test.ts \
  src/pages/secrets/dialogs/write.test.tsx \
  src/pages/secrets/libsecret/helpers.test.ts \
  src/pages/secrets/libsecret/dialogs/create-item.test.tsx \
  src/pages/secrets/reveal-secret-view.test.tsx
```

## SPA browser tests

Playwright exercises protected routing, authentication, CSRF, stores, secrets,
permissions, client instances, and user workflows. Authentication coverage
includes hard-navigation sign-out with a freshly primed CSRF token, backend
session destruction, stale-cookie rejection, and a failed first logout that
remains visible and authenticated until a successful retry. The resource
lifecycle flow shows a typed libsecret blocker in the still-open delete dialog,
clears it, and then observes successful resource removal.

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
- local and remote-style runtime client enrollment;
- the deployed runtime identity cutover: the packaged `underwrap-keygen`
  successfully installs
  `/var/lib/underwrap-local/client-local.identity.pem` from
  `/etc/underwrap/daemon/local.yaml` with exact `underwrap-local:underwrap-local`
  ownership and mode `0400`; the unchanged daemon PID still succeeds with its
  cached old identity before the public-certificate backend update; an exact
  comparison of `id`, `display_name`, `current_store_id`, `current_store_name`,
  `status`, `auto_create_wrapper_assets`, and
  `current_credential_fingerprint` shows that only the fingerprint changes; the
  same PID then receives enrollment guidance; and an explicit
  `underwrapd-local.service` restart yields a distinct nonzero PID and bounded
  wrapper success using the replacement identity.

That cutover witness preserves the exact keygen → public-certificate PUT →
same-PID cached-old failure → restart → distinct-PID success order. It discards
target output, does not read the private-key portion separately, and passes only
the public certificate through the authenticated admin request-body helper. It
excludes volatile `last_seen_at` from the client-record comparison and leaves
the replacement active for subsequent direct runtime mTLS checks. Those checks
also give curl the same combined identity path for both `--cert` and `--key`.
The witness establishes the explicit-restart operation and interruption window,
not live reload, unauthorized-triggered retry, rollback, a grace credential, or
zero downtime.

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

The admin-reference tests establish deterministic behavior for serialized
requests only. They do not exercise or claim cross-request transactions, CAS,
locks, tombstones, concurrent check/use exclusion, or repair/migration of
pre-existing inconsistencies; those limits remain operational requirements in
[Backend](backend.md#concurrency-semantics). Registry failure tests likewise
establish returned-helper-error and running-process publication behavior, not
parent-directory fsync, power-loss durability, cross-registry serialization,
pre-registry create-side-effect cleanup, or automatic repair.

Security-sensitive changes still require human review against
[Security](security.md).

## Related documentation

- [Architecture](architecture.md)
- [Security](security.md)
- [`AGENTS.md`](../AGENTS.md)
