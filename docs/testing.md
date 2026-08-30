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

`cargo test -p underwrap-protocol config` covers the daemon configuration grammar at runtime-validation boundaries.
It exercises exact instance and wrapper name lengths, reserved `debug`, the complete privileged path-field inventory, canonical relative files paths, canonical absolute and tilde access-path domains, same-domain duplicate and overlap rejection, forced path environment values, and retained unrestricted resource, URL, arbitrary environment, and `PATH` contracts.

For cross-component daemon/backend behavior:

```bash
./crates/integration-tests/run
```

The runner supports `x86_64-linux` and `aarch64-linux`. It resolves the current
system through Nix, builds exactly that system's `web-build` check,
`packages.<system>.underwrapd`, and `packages.<system>.underwrap-backend` without
creating result links, verifies `index.html` plus each package-specific
`/bin/<name>` executable, and runs the crate in the pinned development shell. It
supplies only those three prerequisite paths to the tests and preserves an
explicitly set `CARGO_TARGET_DIR`; additional arguments are forwarded to
`cargo test` after the fixed package selector.
Host capability-dependent skips remain distinct from these mandatory build
prerequisites.

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

### Protocol profile isolation

The protocol's default test command exercises the unchanged `full` profile:

```bash
cargo test -p underwrap-protocol
```

The namespace wrapper selects the explicit `local-abi` profile, which can be checked independently with:

```bash
cargo check --locked -p underwrap-protocol --no-default-features --features local-abi
cargo test --locked -p underwrap-protocol --no-default-features --features local-abi
cargo clippy --locked -p underwrap-protocol --no-default-features --features local-abi -- -D warnings
cargo test -p underwrap-ns
```

Use a fresh target directory when module-compilation evidence must be independent of prior artifacts:

```bash
target_dir="$(mktemp -d)"
CARGO_TARGET_DIR="$target_dir" cargo check --locked -p underwrap-ns
rg -l 'src/(config|control|entities|policy|service|scm_rights)(/|\.rs)' \
  "$target_dir"/debug/deps/underwrap_protocol-*.d
rm -rf "$target_dir"
```

The `rg` command must print no dep-info path because the isolated protocol compilation includes only `access`, `exec`, `ipc`, `libsecret_publication`, `message`, `resource`, `resource_blob`, `target_env`, and `variant`.
Inspect the isolated package feature and normal/build dependency trees with:

```bash
cargo tree --locked -p underwrap-ns -e features --depth 2
cargo tree --locked -p underwrap-ns -e normal,build --depth 2
```

The feature tree must show `local-abi` without protocol `default` or `full`.
The normal/build tree must contain no protocol-owned Tokio, Utoipa, Serde YAML, or libc edge, while retaining `underwrap-ns`'s direct libc dependency.
These commands establish isolated package compile and review coupling only.
They do not establish a reduced runtime trusted computing base or workspace-wide feature isolation because Cargo can unify features when a build contains a full-profile consumer.

## Deterministic repository checks

Flake package, check, and development-shell outputs support exactly
`x86_64-linux` and `aarch64-linux`. The flake declares these checks on both
systems:

```bash
nix build .#checks.x86_64-linux.workspace-test
nix build .#checks.x86_64-linux.workspace-nextest
nix build .#checks.x86_64-linux.workspace-clippy
nix build .#checks.x86_64-linux.workspace-fmt
nix build .#checks.x86_64-linux.workspace-audit
nix build .#checks.x86_64-linux.workspace-deny
nix build .#checks.x86_64-linux.workspace-llvm-cov
nix build .#checks.x86_64-linux.web-build
nix build .#checks.x86_64-linux.web-test
nix build .#checks.x86_64-linux.examples-nixos-eval
nix build .#checks.x86_64-linux.frontend-backend-url-eval
nix build .#checks.x86_64-linux.frontend-security-assertions-eval
nix build .#checks.x86_64-linux.frontend-swap-posture-eval
```

`web-build` is the production SPA artifact embedded in the Rust package.
`web-test` runs the complete Vitest suite; both use one npm dependency closure
pinned by the committed lockfile and flake hash. `examples-nixos-eval` evaluates
and builds both shipped examples as complete NixOS systems through the real
underwrap module and selected system package.

Run the complete deterministic flake gate for the current supported system
with:

```bash
nix flake check
```

This aggregate covers the listed Rust tests, formatting and lint, pinned
RustSec advisory snapshot, dependency policy, coverage generation, SPA unit and
production builds, example systems, and focused NixOS module evaluators. On
`x86_64-linux` it additionally includes the production VM check:

```bash
nix build .#checks.x86_64-linux.vm-test
```

The VM check is intentionally not exposed on `aarch64-linux`. Evaluate every
supported system's outputs without cross-building them with:

```bash
nix flake check --all-systems --no-build
```

The direct SPA commands remain useful for a focused local loop:

```bash
cd web && npm test && npm run build
```

Playwright is a separate host/browser integration suite and the live npm audit
is a separate advisory-service query; neither is part of the sandboxed flake
aggregate.

## Dependency audits

Audit the npm lock separately from the aggregate flake checks:

```bash
cd web && npm audit --package-lock-only
```

The pinned RustSec snapshot used by `workspace-audit` currently reports zero
fixable findings and exactly two unmaintained notices without patched releases:
`paste` (`RUSTSEC-2024-0436`) and `rustls-pemfile` (`RUSTSEC-2025-0134`). Node is
build and development tooling for the SPA; the backend embeds the resulting
static assets, and Node does not enter its runtime closure. Keep the direct
`@playwright/test` package version aligned with the nixpkgs Playwright driver
when updating nixpkgs so browser revisions remain compatible.

These audits describe the pinned lockfiles and advisory snapshots at the time
they run. They do not prove that dependencies are free of undisclosed or future
vulnerabilities, nor that an affected package path is exploitable by underwrap.

## Rust toolchain

The flake explicitly pins Rust 1.97.1 for development shells, Crane packages,
and checks while retaining the `rust-src`, `rust-analyzer`, and
`llvm-tools-preview` components. Verify the complete toolchain selection with:

```bash
nix develop -c bash -lc 'rustc --version; rustc -Vv; cargo --version; cargo clippy --version; rustfmt --version'
```

Rust 1.97.1 is the fixed release selected to move release builds beyond the
known LLVM optimizer defect range that included the previous reproducible
compiler. Rebuilding and testing with the fixed compiler removes that avoidable
toolchain uncertainty; it does not establish that any prior underwrap artifact
was miscompiled or add a runtime security guarantee.

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
- daemon peer authentication and wrapper intake, including a private injected
  operation seam shared by the live resolver. Deterministic traces prove that
  only `SO_PEERPIDFD`'s `ENOPROTOOPT`/unsupported result reads the
  `SO_PEERCRED` PID's status and selects `PeerCredFallback`; all other
  acquisition errors, malformed descriptor payloads, pidfd PID/status
  resolution failures, malformed status, and credential mismatches reject
  without fallback. Every accepted pidfd trace closes exactly once after its
  final resolution/status use, including all rejection outcomes;
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

### Daemon resource-open parser evidence

The focused protocol resource-blob tests cover empty and populated typed round trips, byte-stable canonical re-encoding, independently built equality, sorted map serialization, Unicode, arbitrary non-UTF-8 secret bytes, empty sections, unknown schema rejection, garbage JSON rejection, inclusive complete-resource acceptance at one byte below and exactly 2 MiB, and size-before-parse plus serialization rejection at one byte above 2 MiB.
The timestamp vectors below add exact safe-integer field and parse-order coverage.
Strict libsecret publication tests separately cover exact canonical bytes, defaults and unknown or duplicate struct fields at that stricter boundary, identifier and alias-reference semantics, and the inclusive 1-MiB publication cap; they do not turn those checks into daemon-open semantics.

The current `underwrapd` tests decode valid invocation-open `Deny` responses in the runtime-client concurrency fixtures and exercise the request deadline and admission behavior, but they do not drive a secret-bearing `Allow` response through `wrapper_flow/read.rs`.
The integration runner exercises valid backend `Allow` responses through the real daemon parser, original-byte channel cache, Checkout framing, and receiver on ordinary service-mode reads, a resource above the generic 64-KiB frame limit, exact-2-MiB Checkout and Checkin, writable and read-only finalization, and follow-up Checkout.
The direct runtime API integration tests additionally pin backend open and finalize behavior, but direct endpoint tests are not daemon parser tests.

There is no dedicated daemon malformed-backend-open matrix for invalid outer JSON, malformed or oversized `blob_b64`, invalid typed resource JSON, unknown/default/duplicate-field behavior, conditional env-name rejection, noncanonical accepted bytes, or the 1-MiB libsecret generation-readiness consequence.
The repository has no dedicated cargo-fuzz or libFuzzer harness, fuzz target, corpus, or recorded fuzz execution for this boundary, and it does not use proptest or quickcheck here.
The valid-flow and protocol vectors therefore do not establish malformed-input or fuzz completeness.

Use these focused tests and static parser/copy inventories when reviewing this boundary:

```bash
cargo test -p underwrap-protocol resource_blob::tests
cargo test -p underwrapd
./crates/integration-tests/run
rg -n 'open_request|open_invocation|blob_b64|blob_bytes|from_canonical_bytes' \
  crates/underwrapd/src/{backend_client.rs,wrapper_flow/read.rs}
rg -n 'SecretBytes|cached_blob|CheckoutOk|read_canonical_resource_bytes|MAX_WRAPPER_RESOURCE_FRAME_SIZE' \
  crates/underwrap-protocol/src crates/underwrapd/src/wrapper_resource_channel.rs
rg -n 'cargo-fuzz|libFuzzer|fuzz_target!|proptest|quickcheck' \
  --glob '!docs/**' --glob '!tickets/**' .
find . \( -path ./.git -o -path ./target -o -path ./result \) -prune -o \
  -type d \( -name fuzz -o -name fuzz_targets -o -name corpus \) -print
```

The static inventory follows the owned HTTP DTO and base64 string, decoded resource vector, temporary typed `SecretBytes`, original-byte cache, Checkout copy, encoded payload, bounded receiver frame, and decoded inner copy.
`SecretBytes` vectors in the temporary parsed resource zeroize on drop, but tests do not instrument post-drop memory and the ordinary HTTP/base64 storage, cache, Checkout/frame vectors, receiver buffers, metadata, allocator remnants, dependency allocations, and operating-system copies remain outside that mechanism.
This evidence does not establish comprehensive zeroization or resolve the broader copy-erasure gap.

Protocol resource-blob timestamp tests exercise exactly 32 boundary outcomes: four fields (`Collection.created`, `Collection.modified`, `Item.created`, and `Item.modified`) × two containers (`ResourceBlob` and `LibsecretSection`) × typed maximum encode success, independently formatted raw maximum decode success, typed maximum-plus-one encode rejection, and independently formatted raw maximum-plus-one decode rejection.
Direct semantic-validation and strict-publication tests retain the same value-free error message, accept the inclusive maximum without temporal-order rules, and reject each field independently above the maximum.
Mixed-defect tests pin complete-resource size before parsing, publication-body size before parsing, timestamp decoding before schema, canonical, or publication-semantic checks, and timestamp encoding before a post-serialization resource-size check.
Those protocol tests establish the four-field encode/decode boundary and validation order only; they do not establish timestamp generation, clock correctness, future-time policy, or collection/item modification propagation.

The item-setter timestamp matrix uses distinct deterministic backdated item and collection baselines without sleeps or exact wall-clock expectations.
Store tests cover `set_secret`, `set_attributes`, and `set_label`, requiring each successful setter to change only its requested fields, preserve created and unrelated data, and assign one exact shared value above both baselines to the item and containing collection `modified` fields.
The same store matrix covers missing collections and items for all three setters, requiring `false` and exact full-state preservation.
Item handler ACK tests independently cover secret, attributes, and label mutations on fresh backdated stores, requiring exact published/accepted metadata equality, preserved created and unrelated fields, a shared item/collection timestamp above both baselines, and the exact same value from the `Item.Modified` getter.
The three matching REJECT tests require the published candidate's item and collection timestamps to be equal while the complete accepted live store remains structurally unchanged, and the EOF test retains separate transport-failure evidence.
The table-driven `item_setters_update_collection_modified_property` test invokes all three public store setters on fresh backdated stores and requires `Collection.Modified` to return the exact shared item/collection value without a D-Bus wire harness or signal assertion.
This matrix establishes setter propagation and ACK-gated live-state behavior for these paths only; it does not establish clock correctness, temporal validity of loaded timestamps, monotonicity, subsecond ordering, signal delivery, caller response delivery, or backend durability.

Focused store-boundary evidence uses strict package Rustdoc plus static dependency and production call-path inventories:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps -p underwrap-secretsd-store
cargo tree --locked -p underwrap-secretsd-store -e normal --depth 1
rg -n 'SystemTime|getrandom' crates/underwrap-secretsd-store/src
rg -n 'store_from_section|section_from_store|publish_store|candidate\.commit' \
  crates/underwrap-secretsd/src/{main.rs,section_store.rs,publication.rs,dbus_service/mutation.rs}
```

These checks assign collections, items, aliases, timestamps, raw bytes, and in-process time and randomness to `underwrap-secretsd-store`.
They assign one-shot initialization conversion, candidate conversion, canonicalization, private publication, external acknowledgement, and commit ordering to the production `underwrap-secretsd` caller.
[Architecture](architecture.md) and the [Secret Service input boundary](secretsd-security.md) are normative for the complete mechanism and security boundary.
Static ownership evidence does not prove that callers or configured targets cannot persist or disclose data, or that accepted backend generations cannot later be persisted or disclosed.

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
relay dispatch seam, and libsecret publication framing/selection.
The libsecret helper-child tests pin both exact endpoint matrices, retained gate CLOEXEC, selected-endpoint CLOEXEC clearing and usability, controlled-error and caught-panic `_exit` outcomes without return, and a reused descriptor that remains live and readable through injected unwind.
The gate, status, and publication channels are independent evidence; none authorizes a lifecycle by itself.
The VM independently covers the deployed privileged chain,
relay/PID-2 topology, supervisor/relay termination, complete namespace teardown,
status/writeback preservation, old same-UID pathname irrelevance, legitimate
mutation persistence, near-cap publication persisted through Checkin and
subsequent D-Bus readback, exact-2-MiB files round trips, rejected oversize
admin/files mutations with byte-identical prior storage, host-only
receiver/secretsd-only publisher ownership, and plaintext/exact-base64 log
hygiene.

Keygen NSS tests inject unavailable, zero, distinct negative, unrepresentable, and oversized buffer hints; exact checked growth and exact-maximum `ERANGE`; arithmetic and initial/growth allocation failures; `EINTR` and another non-`ERANGE` error; not-found, success, and a foreign result pointer.
They prove that only `ERANGE` grows, non-`ERANGE` errors are not retried, operational failures precede key generation and identity-file mutation, and daemon-owner success/not-found behavior retains its ordering.
Real NSS probes cover root and a missing account.
Synchronized calls through public `Cli::resolve_targets` are a concurrent public-API smoke/stability check: they verify stable results when calls start concurrently, but do not prove overlap inside NSS or the absence of races.

Daemon startup NSS tests inject unavailable, zero, distinct negative, unrepresentable, and oversized group-buffer hints; exact doubling and cap-to-1-MiB growth; success and `ERANGE` at the maximum; checked arithmetic; initial and growth allocation failures; `EINTR` and another non-`ERANGE` error; embedded-NUL, not-found, exact-result, and foreign-result-pointer outcomes.
They pin caller-owned `getgrnam_r` result validation, copied `gr_gid`, ERANGE-only growth, and the exact reservation/request bound.
Real NSS probes cover root and generated missing groups, while a production `server::run` test proves a missing configured group returns before creating the wrapper socket.
These tests exercise underwrap's ordering and response handling; they do not prove NSS-provider implementation details or scheduler behavior.

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

### Sensitive persistence claim evidence

Use these focused checks when reviewing the scoped persistence claim and its separate credential, authentication-record, and configuration controls:

```bash
cargo test -p underwrap-keygen
cargo test -p underwrapd startup::tests::credential_permissions
cargo test -p underwrap-backend users
nix build .#checks.x86_64-linux.frontend-security-assertions-eval
nix build .#checks.x86_64-linux.examples-nixos-eval
nix flake check --all-systems --no-build
```

The keygen suite covers daemon-account, existing-owner, and invoking-identity precedence together with mode-`0400`, file-sync, atomic-persist, and stdout-after-commit behavior.
It does not establish parent-directory fsync, power-loss durability, secure erasure, backend enrollment transactionality, or live daemon reload.
The daemon credential-permission filter covers a missing file, an accepted mode-`0400` file under mode `0700`, rejection of mode `0640`, and rejection of a mode-`0755` parent.
Direct source inspection additionally shows the file-owner check and that parent ownership and mode are checked only when parent metadata lookup succeeds; the focused tests do not exercise a wrong owner or failed parent metadata lookup.
The backend `users` filter covers Argon2 record validation, persisted user mutations, API omission, and `Debug` redaction.
Static inspection of `users.rs` and `storage/json_dir_store.rs` shows restrictive Unix `NamedTempFile` creation, file sync, and atomic rename, but neither the tests nor the application establish a final record mode, users-directory mode, parent-directory fsync, encryption at rest, or secure erasure.
Static inspection of `tls.rs`, encrypted-store lifecycle code, and the NixOS module establishes current code shape for TLS-key loading, master-key handling, retained workspaces, generated configuration, and declared service paths only.
The two Nix builds prove that the unchanged production module and examples evaluate through their pinned check definitions, while the all-systems no-build command proves output evaluation for supported systems.
Nix evaluation does not prove deployed ownership, mode, umask, runtime file contents, CA-key custody, secure deletion, memory erasure, or target behavior.
The normative claim and control inventory remains in [Security](security.md#1-underwrap-managed-durable-brokered-resource-material-uses-backend-storage).

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

The complete frontend security assertion matrix in
`frontend-security-assertions-eval` uses real `nixpkgs.lib.nixosSystem`
evaluations of the production module. One valid
hardened instance/wrapper baseline pins the exact sorted inventory of all 18
underwrap assertion messages, requires every module-local and global assertion
to be true, and forces a successful system toplevel.
Seven positive rows cover the exact instance/wrapper length boundaries, canonical root and relative paths, mixed absolute/tilde access domains, exact `~`, ordinary Unicode paths, unrestricted `PATH`, and a `pkgs.writeText` backend CA derivation coerced to its canonical store path; `valid.tsv` records their successful toplevel results.
The 81 option-type-valid negative rows preserve all prior assertion rows and cover every assertion family and independently meaningful conjunct or security-sensitive class within combined assertions.
They include strict instance/wrapper names, runtime-base and CA paths, target-UID hash collision, backend transport, wrapper target/resource/projection/files paths, absolute and tilde access paths, linker and forced environment names, managed and path-like fixed environment values, reserved `debug`, ordinary swap, zram writeback, and the Linux 6.5 floor.
Every negative row requires one exact false underwrap message, exactly one false assertion globally, and a failed forced toplevel.
The deterministic `matrix.tsv` records row, family, message, and toplevel result.

This is fail-closed evaluation evidence for the assertion expressions and the
pinned nixpkgs/NixOS module composition, not runtime-enforcement evidence or a
proof for arbitrary nixpkgs versions. The target-UID collision row pins two
known production naming inputs and their current 12-hex SHA-256 prefix; it does
not analyze collision probability or validate additional configuration input
shapes. The old-kernel row evaluates the pinned `linuxPackages_6_1` package set
and checks the exact resulting dynamic message. Finer valid-output evidence
remains in the existing focused checks and deployed VM.

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

StrictMode hook tests and a shared Secrets/Wrapper-permissions Vitest matrix cover unresolved, stale, removed, valid, failed, and empty store-list states, URL and history preservation, popstate revalidation, detail-query gating, and store-scoped dialog reset.
The configured `desktop-1280` Chromium project covers stale real-backend selection plus intercepted structured-failure and empty-list responses on both per-store pages with exact detail-request observation.
This evidence establishes the tested frontend selection and rendering behavior only; it does not change or prove backend authorization, store availability, or browser behavior outside the configured project.

Playwright exercises protected routing, authentication, CSRF, stores, secrets,
permissions, client instances, and user workflows. Authentication coverage
includes hard-navigation sign-out with a freshly primed CSRF token, backend
session destruction, stale-cookie rejection, and a failed first logout that
remains visible and authenticated until a successful retry. The resource
lifecycle flow shows a typed libsecret blocker in the still-open delete dialog,
clears it, and then observes successful resource removal.

The focused `focus-indicators.spec.ts` run in the configured `desktop-1280` Chromium project observes the shared dialog close control in the desktop Create store flow under the default dark and toggle-driven light themes and the shared sheet close control in the 375×812 responsive layout under the default dark theme.
It reaches each close control through the real keyboard focus path and observes an unfocused `box-shadow: none` baseline, a focused non-`none` box shadow in regular colors, and a `:focus-visible` outline at least two pixels wide under emulated forced colors.
This evidence makes no contrast, WCAG conformance, screen-reader, cross-browser, mobile-browser, or application-wide focus-coverage claim.

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

- effective-GID wrapper-socket peer authentication. The hardened witness first
  asserts the running kernel is Linux 6.5 or newer, snapshots a journal cursor
  immediately before one successful uniquely scoped setgid `test-echo`
  invocation, and decodes only post-cursor `underwrapd-local` JSON records. Its
  matching wrapper-read record must report `auth_path: PeerPidfd`, while the
  complete scoped record set must contain neither `PeerCredFallback` nor a
  fallback warning;
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

The private-libsecret-bus witness runs one dedicated target in a uniquely named
transient systemd unit. The target completes a real lookup with its output
discarded, reports its filesystem D-Bus address, and blocks on a unique release
marker. While that exact unit is active and running, trusted root instrumentation
in the host mount namespace proves that the exact reported pathname is absent
and that no socket exists below the host bus tree. Separately, a process owned by
the original caller performs exact metadata access and an `AF_UNIX` stream
`connect_ex` against that pathname; both must return `EACCES` through the
mode-`0000` bare host mountpoint. The unit must still be active and running after
those probes. Only then does the test create the marker; the target completes a
second discarded lookup, reports its fixed post-probe marker, and the unit must
finish with a clean result and status. Exact-unit cleanup in a `finally` block
makes the evidence independent of ordinary teardown timing.

This is evidence for the normal filesystem-backed private bus on the tested
NixOS kernel and deployed wrapper chain. Root's host-namespace absence result and
the same-caller mode-`0000` metadata/connect denials are distinct observations:
the caller's `EACCES` does not itself prove pathname absence. The witness does
not hide or probe abstract Unix sockets, establish protection against host root
or kernel compromise, or treat the configured target as untrusted. The target
is trusted with the libsecret projection and can disclose it; the witness only
exercises the same-caller host mount-namespace boundary while that trusted
target and its private services are live.

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

- correctness of the Linux `SO_PEERPIDFD` implementation itself. Unit tests
  prove underwrap's production decision and close ordering through injected
  operation results, while the VM witnesses the selected path on its one
  deployed kernel/invocation; neither is a kernel proof or evidence for every
  kernel/procfs environment;
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
