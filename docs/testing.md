# Testing

Use [CONTRIBUTING](../CONTRIBUTING.md#prepare-a-checkout) for fresh-checkout preparation.
Run commands from the repository root inside `nix develop` unless a command explicitly enters another directory.
Tests provide regression evidence, not a security proof or an independent end-to-end audit.

## Choose checks by change

| Changed boundary | Checks before committing |
| --- | --- |
| One Rust crate | Crate tests, Rustfmt, denied-warning Clippy |
| Runtime DTO, policy, or resource channel | Protocol, backend, and daemon tests; integration runner; namespace tests when affected |
| Admin handler or DTO | Backend tests, OpenAPI drift and semantic checks, SPA tests/build |
| SPA behavior | Vitest and build; selected coverage gate for API/library work; Playwright for user flows |
| Storage or encryption | Store/backend tests and real daemon/backend integration |
| NixOS module | Focused evaluators, shipped examples, relevant workspace Nix checks, VM |
| Entry chain, peer auth, identities, namespaces, capabilities, mounts, or fd lifecycle | Relevant crate and integration tests plus VM |
| Documentation or examples | Links, paths, example parser/evaluator checks, affected generated-contract checks |

Unavailable checks remain explicit gaps, not passes.
Review security-sensitive changes against [Security](security.md) and the joint source-review requirements in [AGENTS.md](../AGENTS.md).

## Rust and integration commands

For a prepared checkout, use focused tests while iterating:

```bash
cargo test --locked -p underwrap-protocol config
cargo test --locked -p underwrap-protocol resource_blob::tests
cargo fmt -- --check
cargo clippy --locked --all-targets --workspace -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --locked --workspace --no-deps
```

The last two workspace commands need the populated `UNDERWRAP_WEB_DIST` from checkout preparation.
The direct Rustdoc command explicitly supplies `--locked`; the pinned Crane `workspace-doc` check supplies it and `--no-deps` through its defaults.

The integration runner builds the exact current-system prerequisites without result links and forwards arguments after its fixed package selector:

```bash
./crates/integration-tests/run
./crates/integration-tests/run harness::tests
./crates/integration-tests/run tests::store_migration
```

It accepts only `x86_64-linux` and `aarch64-linux`, verifies the SPA index and each package-specific executable, and preserves an explicitly set `CARGO_TARGET_DIR`.
Host capability-dependent skips are separate from these mandatory prerequisites.
A test-harness executable is not a substitute for either packaged service binary.

To run the raw workspace suite, prepare all three artifacts explicitly:

```bash
system=$(nix eval --impure --raw --expr builtins.currentSystem)
web_dist=$(nix build --no-link --print-out-paths ".#checks.${system}.web-build")
daemon_package=$(nix build --no-link --print-out-paths ".#packages.${system}.underwrapd")
backend_package=$(nix build --no-link --print-out-paths ".#packages.${system}.underwrap-backend")
test -f "$web_dist/index.html"
test -x "$daemon_package/bin/underwrapd"
test -x "$backend_package/bin/underwrap-backend"
env "UNDERWRAP_WEB_DIST=$web_dist" \
  "CARGO_BIN_EXE_underwrapd=$daemon_package/bin/underwrapd" \
  "CARGO_BIN_EXE_underwrap-backend=$backend_package/bin/underwrap-backend" \
  cargo test --locked --workspace
```

### Isolated local ABI

The protocol default is `full`; `underwrap-ns` disables defaults and selects `local-abi`.
Check that profile separately from a workspace build:

```bash
cargo check --locked -p underwrap-protocol --no-default-features --features local-abi
cargo test --locked -p underwrap-protocol --no-default-features --features local-abi
cargo clippy --locked -p underwrap-protocol --no-default-features --features local-abi -- -D warnings
cargo test --locked -p underwrap-ns
cargo tree --locked -p underwrap-ns -e features --depth 2
cargo tree --locked -p underwrap-ns -e normal,build --depth 2
```

The isolated feature tree must select `local-abi` without protocol `default` or `full`.
Its protocol-owned Tokio, Utoipa, Serde YAML, and libc edges are absent; the namespace package retains its direct libc dependency.
When inspecting dep-info for module-compilation evidence, use a fresh `CARGO_TARGET_DIR` and check that protocol `config`, `control`, `entities`, `policy`, `service`, and `scm_rights` sources are absent.
This reduces isolated compile and review coupling, not the runtime trusted computing base, binary size, or exploitability set.
Cargo can unify features when a build includes a full-profile consumer.

## Deterministic Nix gate

```bash
nix flake check
nix flake check --all-systems --no-build
```

The first command builds the deterministic checks for the current supported system.
The second evaluates both supported systems without cross-building them and is not runtime evidence.
The [flake](../flake.nix) declares these independently selectable checks:

| Checks | Scope |
| --- | --- |
| `workspace-test`, `workspace-nextest` | Workspace tests with SPA and packaged service prerequisites |
| `workspace-clippy`, `workspace-fmt`, `workspace-doc` | Lint, formatting, warning-free Rustdoc |
| `workspace-audit` | Pinned RustSec advisory snapshot |
| `workspace-deny` | Dependency bans and sources, not a license-policy gate |
| `workspace-llvm-cov` | Rust coverage report generation |
| `web-build`, `web-test` | Production SPA and complete Vitest suite with selected coverage thresholds |
| `examples-nixos-eval` | Both shipped NixOS examples through the real module and package, including their system builds |
| `frontend-backend-url-eval`, `frontend-security-assertions-eval`, `frontend-swap-posture-eval` | Focused production-module predicates and generated configuration |
| `wrapper-discovery-eval` | Public metadata allowlist, descriptions/defaults, unchanged execution configuration, helper exposure and portable skill packaging |
| `runtime-tmpfiles-eval` | Admitted literal runtime bases through the actual module, daemon configuration and tmpfiles parser with inert `--dry-run --create` |

Select one check with the current system, for example:

```bash
system=$(nix eval --impure --raw --expr builtins.currentSystem)
nix build ".#checks.${system}.workspace-doc"
nix build ".#checks.${system}.web-test"
```

The production VM check exists only on `x86_64-linux` and is included in that system's aggregate:

```bash
nix build .#checks.x86_64-linux.vm-test
nix build .#checks.x86_64-linux.runtime-tmpfiles-vm-test
```

Both run the complete shipped-chain suite, including files-only descriptor-staging and libsecret startup/lookup regressions.
The second uses a custom literal base containing percent, backslash and both quote characters; activation checks exact base/child paths, ownership/modes and absence of differently interpreted paths.
The ordinary custom base remains the first check's control; the assertion evaluator retains exact `/run/user` rejection controls.
Host parser checks are dry-runs, never root-host activation.

It needs a suitable VM builder with the required virtualization support.
AArch64 package/check declarations do not establish AArch64 runtime validation.
The deployed Rust artifacts are the actual dynamically linked Nix packages installed through NixOS security wrappers, not an inferred static or musl build.
Playwright and live npm advisory queries are deliberately outside the sandboxed flake aggregate.

## Browser and live advisory checks

Use a non-root test user inside `nix develop` for the real filesystem-permission failure scenario.
Prepare the local SPA and backend from the same changed tree, with an explicit binary path that also works when another `CARGO_TARGET_DIR` is configured:

```bash
npm --prefix web ci
npm --prefix web run build
CARGO_TARGET_DIR="$PWD/target/browser-tests" \
  UNDERWRAP_WEB_DIST="$PWD/web/dist" \
  cargo build --locked -p underwrap-backend
export UNDERWRAP_BACKENDD_BINARY="$PWD/target/browser-tests/debug/underwrap-backend"
(cd web && npm run test:e2e-support && npm run test:e2e)
```

`UNDERWRAP_BACKENDD_BINARY` is the fixture's exact variable name.
The browser fixture errors if its backend binary or local `web/dist/index.html` is missing; it does not skip those prerequisites.
The development shell selects Nix-compatible browsers and driver Node without downloading Playwright browsers.
For interactive execution, use `npm --prefix web run test:e2e:ui` after the same preparation.

The fixture runs disposable HTTPS backends with generated test PKI and offline operator provisioning.
Its configured certificate-error bypass is test infrastructure, not a production trust policy.
Playwright retains traces and screenshots on failure, so artifacts may contain rendered test key material; do not publish them unreviewed or use live secrets.

Run dependency-health queries separately:

```bash
(cd web && npm ls --all && npm audit --package-lock-only)
(cd web && npm outdated)
```

`npm audit` queries a live advisory service; `npm outdated` can return nonzero when newer versions exist.
Review results against the selected compatibility policy rather than treating all newer majors as supported or every advisory as a reachable exploit.
Neither a pinned snapshot nor a clean live scan establishes absence of undisclosed or future vulnerabilities.

## Toolchain and dependency policy

Rust and Cargo are pinned exactly to **1.97.1** for Nix development, checks, and release builds, and every edition-2024 workspace package declares `rust-version = "1.97.1"`.
That minimum does not promise independent validation of newer compilers.

Cargo resolver **2** is deliberate.
`Cargo.lock` selects dependencies, while resolver 2's allow behavior exposes a newly selected dependency's higher Rust requirement at the exact pinned build instead of resolver 3 falling back to an older compatible version.
Resolver 2 itself does not enforce package `rust-version` during selection; the pinned build supplies that enforcement evidence.
Changes require synchronized manifests, lock evidence, Nix tooling, and support documentation.

```bash
cargo metadata --locked --format-version 1 --no-deps
cargo tree --workspace --locked --edges normal,build,dev
rustc --version
rustc -Vv
cargo --version
cargo clippy --version
rustfmt --version
```

The flake retains `rust-src`, `rust-analyzer`, and `llvm-tools-preview`.
The committed npm lock and Nix inputs define the selected frontend matrix, not every version permitted by a caret constraint:

| Layer | Selected compatibility baseline |
| --- | --- |
| Build tools | Node 24.22.2, npm 11.19.1 |
| SPA | React/React DOM 19.3.1; Vite 8.3.8; React plugin 6.3.0; Tailwind and Vite plugin 4.3.5 |
| Type checking | TypeScript 5.9.3; openapi-typescript 7 requires a `^5.x` peer |
| Unit tests | Vitest/coverage-v8 4.1.12; jsdom 30.1.3; Testing Library versions selected by the lock |
| Browser tests | Exact `@playwright/test` 1.64.1 paired with nixpkgs driver 1.64.1; driver Node 22.23.5 via `PLAYWRIGHT_NODEJS_PATH` |
| API tools | Utoipa 6.0.0; utoipa-axum 0.3.0; OpenAPI 3.1.0; openapi-typescript 7.13.0; openapi-fetch 0.17.0 |

Playwright's exact package pin prevents npm from selecting protocol code incompatible with the pinned browser driver.
OpenAPI 3.1.0 is the generator's valid feature line; a newer published specification is not by itself a migration requirement.
Node builds the static SPA but does not enter the backend's runtime closure.
Lock refreshes require peer-resolution and outdated review, audits, generated-schema checks, unit tests, production build, and the configured browser suite.
Do not silently widen support, lower a gate, or expand exclusions to accommodate instrumentation changes.

## Selected SPA coverage gate

```bash
npm --prefix web run generate:api
npm --prefix web test
npm --prefix web run test:coverage
npm --prefix web run build
```

The V8 gate selects hand-written `src/api/**/*.ts` and `src/lib/**/*.ts`, excluding test files and generated API types from executable coverage.
It requires **80 percent each for statements, branches, functions, and lines**.
The existing Nix `web-test` runs `test:coverage` once for the complete suite; it does not duplicate Vitest in another aggregate derivation.

Add meaningful endpoint/helper assertions: method, URL, path parameters, query, request body, credential posture, response decoding, and error outcomes.
Calling functions solely to raise a percentage is not acceptance evidence.
React Testing Library and Playwright retain their own component/browser scope rather than expanding this gate to all rendering code.
Coverage is neither whole-application or Rust coverage nor correctness or security proof.
Rust LCOV generation is checked separately, and byte reproducibility of its execution-count report under `nix build --rebuild` is not established.

## Generated contracts and source inclusion

After handler annotation or DTO changes, with the SPA prerequisite prepared:

```bash
cargo test --locked -p underwrap-backend --test openapi_export --test admin_reference
npm --prefix web run generate:api
npm --prefix web run build
```

The Rust drift test **rewrites tracked** `crates/underwrap-backend/openapi.json` and fails when it differs from generation.
Review the diff, rerun the test, and commit intended schema changes with their source.
Do not run that test in an otherwise read-only review without authority to handle its mutation.
Npm generation writes ignored `web/src/api/schema.ts`, which is local build output rather than a tracked contract.

Semantic tests traverse every OpenAPI method and check effective security inheritance, public operations, session-and-CSRF requirements in one AND-shaped object, unavailable-store and lifecycle responses, key headers, and conditional key schemas.
`admin_reference` additionally checks selected descriptions for deletion guards, optional force, flat-slot absence, concurrency, and key-owner lifetime.
These selected assertions, byte equality, and structural checks do not prove every description agrees with handlers; inspect real statuses, defaults, null handling, and failure semantics separately.
Client access override semantic tests also pin strict variant properties and optional/null update behavior.
A stale description does not necessarily break TypeScript compilation.

Compile-time non-Rust fixtures must survive both Cargo packaging and the Nix/Crane source filter.
For example, integration `testdata/**` is explicitly included in both its manifest and `flake.nix`; assets, shipped examples, and committed OpenAPI also have filter support.
`cargo package --list --allow-dirty -p integration-tests` can inspect Cargo's inclusion without reading fixture contents, but does not prove Nix inclusion.
Ensure intended new files enter Git's flake source and validate the corresponding sandbox build; a local test can pass while its filtered fixture is absent.

## Fixture and observation discipline

Own temporary state, listeners, tasks, and child processes immediately after acquisition, before another fallible setup step.
Preserve the primary startup or fixture-use error if cleanup also fails.
Browser cleanup observes exit, waits boundedly after TERM and then KILL, and retains temporary state if the child cannot be reaped.
The Node support suite simulates these paths; it does not prove OS signal delivery.
Rust fixtures distinguish daemon abort-on-drop from synchronous backend-thread join and eventual temporary-directory cleanup.
Await normal shutdown rather than wrapping cancellation-unsafe blocking work in an in-runtime timeout; the invoking process supplies the hard whole-suite bound.
Packaged Rust fixtures use standard child handles with nonblocking pipe capture and borrowed synchronous/asynchronous output waits.
Their owner retains shared temporary state immediately after spawn, observes direct-child exit/reap on fallback, and leaks the state reference with a metadata-only diagnostic if reap cannot be confirmed within five seconds.
Failure witnesses run selected packaged paths in fresh externally supervised processes, bind exit observation to pidfds before injecting failure, and check owner-mediated reap while state still exists plus listener closure and eventual directory release.
These services do not launch descendants; this owner is not a generic process-tree supervisor.
Simulated cleanup-observation failures exercise retention/error precedence without intentionally orphaning a process.
These fixture guarantees are not production shutdown semantics or universal cleanup after runtime termination.

Effective-group fixture selection uses privately owned `getgrgid_r` record/name storage through the owned UTF-8 copy, with checked ERANGE doubling from 16 KiB to 1 MiB.
Missing, invalid or failed lookup rejects fixture setup rather than selecting a numeric or different identity.
Concurrent lookup/fixture stress supplements the pointer/lifetime argument, not a sanitizer or race-freedom proof.

The Rust integration backend startup reservation retains an async process-local mutex and CLOEXEC advisory file lock through HTTPS readiness.
It coordinates cooperating Cargo/nextest processes sharing the same temporary directory and effective UID.
An unrelated process can still claim the released probe port; this is not a universal kernel reservation.
Admin and enrolled-runtime HTTPS readiness each use one 60-second elapsed deadline, limiting each request and sleep to the remaining budget and checking backend completion between probes.
Elapsed stalled-TLS and prompt-success controls run under a separate process deadline; a failed backend is observed without timing out its join.
The caller keeps the reservation through readiness failure and backend cleanup, then releases it by ordinary scope/drop ownership.
The readiness budget bounds cancellable probing, not blocking setup, thread joins, normal awaited shutdown or the complete fixture lifetime.

Run numeric-fd close and EOF-sensitive witnesses in fresh single-test processes when parallel reuse or pre-exec inheritance would invalidate observation.
Keep exact `EBADF`, endpoint ownership, CLOEXEC, and EOF assertions rather than inserting delays or retries.
The restrictive-umask rotation child keeps `umask 0700` throughout the operation and assertions; its child-local exit guard changes only the exit umask to `0077` so LLVM can create an owner-readable profile.
Changing the operation's umask would weaken the test, while changing only exit instrumentation does not.

VM canonical-byte observation uses `nix/tests/test.py::admin_get_blob` to transfer independent **64-KiB chunks** with `machine.execute`, avoiding automatic secret-output logging on command failure.
It validates base64 and each expected length, assembles the bytes, and compares length/SHA-256 with guest fingerprints before and after observation.
Changes or truncation fail immediately without retries, and the final canonical byte-equality assertion remains intact.
A short driver observation is not by itself backend corruption; the precise lower-layer truncation cause is not established.
Never print resource bytes, decoded buffers, key material, or failing secret-bearing assertions for diagnosis.

## Security witnesses and limits

A recorded checksum or success entry does not recreate an unavailable temporary validation artifact.
Current reproducibility claims need retained source and inputs plus a fresh corresponding check, not the checksum alone.

The meaningful negative controls live in the tests rather than a documentation assertion inventory.
Preserve these distinctions when extending them:

- Resource tests retain inclusive 2-MiB complete-resource, independent 1-MiB publication, and 64-KiB control domains, exact wire envelope headroom, trailing-byte rejection, and fail-before-effect ordering.
- Valid real-backend `Allow` flows and forced-env rejection exercise daemon parsing/cache/channel behavior, but no dedicated complete malformed-daemon-`Allow` matrix or fuzz harness establishes malformed outer JSON/base64, default/duplicate/noncanonical input, or publication-readiness completeness.
- Typed Secret Service handler tests establish decoded-value validation, exact error precedence, full accepted-state equality, registration outcomes, and ACK-gated commit, not D-Bus wire signatures, malformed-frame decoding, or full draft conformance.
- Secret Service `dbus_service::wire_tests` exercise actual exported-path dispatch, dynamic aliases, accepted/rejected deletion, retained setters, bounded export reuse and registry/property lock ordering over pre-authenticated peer sockets.
  Run the same witnesses against a disposable real bus with `UNDERWRAP_TEST_DBUS_DAEMON=$(command -v dbus-daemon) cargo test --locked -p underwrap-secretsd dbus_service::wire_tests` in the prepared shell.
  The fixture waits for the daemon's printed address and object-server dispatcher readiness and owns kill/reap cleanup; its synthetic matching ACK/REJECT peer is not the namespace receiver or backend durability evidence.
  Isolated child witnesses check post-ACK registration-collision failure and cancellation of a genuinely pending reconciliation; they do not establish recovery or universal D-Bus conformance.
- Secret-omission tracing tests must emit and observe a harmless capture-control event after subscriber installation, including when metadata tracing is enabled; an empty or disabled sink is not non-disclosure evidence.
- Candidate/owned-buffer wiping and static copy inventories do not instrument post-drop memory or establish comprehensive erasure of HTTP, base64, cache, frame, allocator, dependency, or OS copies.
- Peer-auth injected traces permit the explicit weak fallback only for `SO_PEERPIDFD`'s unsupported `ENOPROTOOPT` result; other acquisition, descriptor, status, and credential failures reject without fallback, with exact close ordering.
  The deployed VM witnesses `PeerPidfd` on its selected kernel, not Linux UAPI correctness for every environment.
- Exact generic HTTP error envelopes and type-only logs are distinct from individually audited detailed-source diagnostics; a representative real permission-denied browser case does not cover every route or middleware error.
- Credential tests cover selected file and parent modes, not a complete wrong-owner/failed-parent-metadata matrix or production key custody.
- Reference and registry rejection tests retain exact structured envelopes, full-state snapshots, no-orphan/operation traces, and successful reuse after rejection; a 4xx or 5xx alone does not establish fail-before-effect behavior.
- Registry and migration fault hooks prove selected returned-error, state, ordering, rollback, and cleanup outcomes, not transactions, CAS, crash/power-loss durability, secure media erasure, or operator quiescence.

The VM boots the real NixOS accounts, setgid/capability wrappers, target UIDs, daemons, and backend services.
It witnesses caller-to-target `/proc` denial, lifecycle teardown, private projections, idmapped access, policy/writeback, size limits, and explicit credential-replacement restart through the deployed chain.
Gate/status/publication evidence remains conjunctive: a local ACK is not D-Bus caller receipt or backend persistence, and target status is not a writeback receipt.
See [Local protocol](local-protocol.md) and [Backend](backend.md) for those contracts.

The live private-bus test requires readiness and a real lookup before host probes, continued target liveness across them, and a second lookup afterward.
Trusted root's host-namespace pathname absence is distinct from the original caller's metadata/connect `EACCES` through the bare mode-`0000` mountpoint.
Neither observation covers abstract sockets, root compromise, or an untrusted configured target.
Nix negative evaluation requires the exact intended false assertion and failed forced system toplevel derivation path, not an unrelated failure; evaluating only the lazy toplevel attribute set does not force NixOS assertions.
Typed option checks do not attest activation, external drop-ins, or live swap devices.

Journal hygiene covers selected successful paths and four backend/daemon unit journals plus native `underwrap-ns` records.
The test derives plaintext, standard/URL-safe base64 with/without padding, lowercase hex, canonical raw/JSON-escaped text, encoded complete-resource forms, and the env-entry signature in memory, discarding target output and reporting only safe representation labels on failure.
It does not cover target output, all paths or transforms, other sinks, external collectors, or formal noninterference.

The VM is one-node loopback HTTPS/mTLS evidence, not SSH or a separate-host deployment test.
It does not exercise non-loopback DNS/SANs, firewalls, routing, latency, partitions, distinct-host compromise, or multi-machine orchestration.
A disposable deployment walkthrough using API operator transitions is not a browser-through-VM witness.
No physical-device, general AArch64 runtime, external key-custody, or configured-target trustworthiness claim follows from the suite.

## Wrapper discovery evidence

`cargo test --locked -p underwrap-info` checks inventory/detail selection, ambiguity, empty instances, strict versioned decoding, public diagnostics, terminal escaping and JSON preservation.
`wrapper-discovery-eval` evaluates the real module for described and undescribed wrappers, including an instance without alias users and an empty instance.
It checks the exact metadata allowlist, description-independent daemon/sidecar and security-wrapper configuration, helper exposure, and the self-contained exported skill's packaged bytes.
The VM runs discovery as `nobody`, without convenience aliases and before either backend starts, checks explicit instance selection, and temporarily removes/restores only the public catalog to witness missing-catalog failure.
These checks do not execute a representative external service wrapper or establish live authorization, guaranteed path access, agent compliance, or production activation.

## Frontend witness boundaries

### Store selection

The URL `store` parameter is requested state, not an effective selection until validated against the authoritative resolved store list.
Unresolved, failed, and resolved-empty lists remain distinct.
Hook and per-store page tests cover stale/removed selection canonicalization, unrelated URL/history-state preservation, popstate revalidation, delayed lists and StrictMode, detail-query gating, and synchronous reset of store-scoped dialogs and nested state.
The configured desktop Chromium witness observes detail requests on Secrets and Wrapper permissions with real stale selection and intercepted failure/empty responses.
Real-router Setup tests additionally exercise cached enrollment navigation, exact store/client continuation, unresolved-metadata creation gates, failed client refresh, and user history-state preservation through intent consumption and filter changes.
Their API responses are mocked; the harness is not the production authentication/layout boundary.
This is frontend behavior evidence, not backend authorization, store availability, or general browser support.

### Client access overrides

Protocol matrix and registry tests cover strict override shapes, safe defaults, saved modes and writable flags, absent rows, debug approval boundaries, and reload persistence.
Handler tests cover current decisions, failed override persistence, independent identity/store/registration gates, and immutable env; their certificate extensions and unavailable lifecycle states are simulated.
Real HTTPS/mTLS tests change overrides between open and finalize, compare complete canonical resources after accepted and denied file/libsecret-metadata changes, and verify saved permission rows remain unchanged.
Debug expiry is exercised by installing an already-expired row, not waiting across an elapsed deadline.
The real daemon scenario checks its cached read-only Checkout hint and current backend authorization of pending Checkin; it does not execute the namespace's read-only writeback suppression path.
Desktop Chromium exercises immediate controls against a disposable real backend, cancellation before Unlock all approval, server-acknowledged writability, restoration to Individual settings, and unchanged saved permission metadata.
These scenarios do not establish retroactive recall, representative target success, or independent security assurance.

### Confirmation error ownership

`useConfirmMutation` catches expected request rejection and synchronous request throws into handled error outcomes.
Invalidation and configured `onSuccess` run outside that request catch; their failures remain observable promise rejections, and exact success follows their completion.
The hook tests retain retry, discarded request-error outcome, caller inventories, and absence of generic-dialog/global rejection suppression.
Shared form and permission-editor guards span validation, request, awaited invalidation, and completion callbacks; only request rejection becomes an inline submission error.
Real active-query observer regressions require awaited invalidation to reject failed refetches while retaining query error state, and distinguish deliberately non-blocking background reconciliation.
The desktop Chromium scenario intercepts one store DELETE with a synthetic structured 503 and checks the retained dialog, inline error, restored button, and unsuppressed diagnostics after a browser macrotask.
It does not exercise a real backend DELETE rejection or establish application-wide, every-promise, or cross-browser cleanliness.

### Form lifetime

`pages/form-lifecycle.test.tsx` exercises all eleven resource, libsecret, user, store and wrapper/resource-pair form callers under StrictMode with deferred request and invalidation, blocked dismissal/navigation, and fresh-draft controls.
Real-page tests cover store fallback/removal and libsecret refresh-error recovery during each deferred phase; original-address invalidation continues while the unmounted form's success callback is suppressed.
Hook tests retain observable invalidation rejection after unmount, alongside mounted request retry and callback-failure ownership.
The desktop Chromium form-lifecycle scenarios delay a real resource-create request and active refresh, attempt dismissal/navigation/reopening, then check normal completion and a later draft; an intercepted request rejection separately checks inline error and successful retry.
These are selected frontend ownership witnesses, not backend cancellation, exactly-once mutation, forced-closure protection, or every-browser evidence.

### Authentication

Logout tolerantly primes a fresh CSRF token with `whoami` GET before the revoking POST, clearing cache and replace-navigating only after success.
Prime or POST failure preserves cache/session/URL and offers a visible non-overlapping Retry; the StrictMode guard prevents duplicate initial attempts, not cross-tab coordination or automatic retry.
Hard-navigation and stale-cookie replay tests are distinct from already-primed in-app unit evidence.

Password persistence precedes submitting-session refresh; refresh failure can still mean the password changed and require a new login.
Other changed, reset, or deleted-user sessions invalidate lazily on their next admin request, and backend restart loses memory sessions.
One two-context Chromium scenario is not proof for every route, concurrent change, session, or backend.

### Presentation and secret custody

Selected keyboard tests observe the dialog and sheet close controls under dark/light and emulated forced-colors conditions, not contrast, WCAG, screen-reader, or application-wide accessibility conformance.
One WebKit tablet scenario pins viewport and screen to 768×1024, observes contained table overflow, then exercises the 767-pixel navigation transition.
It establishes neither a cards layout nor broad tablet, orientation, zoom, physical-device, or browser support.

Text/attribute tests pin exact UTF-8 conversion, distinct missing/empty/malformed-base64/non-UTF-8 outcomes, first-delimiter parsing, trimmed exact duplicate detection, line errors, and prototype-safe own keys with no request on invalid input.
Leading-BOM helper and shared-view regressions include BOM-only values and invalid UTF-8 controls.
A desktop Chromium real-backend witness compares exact reveal and real clipboard content for env, files and libsecret, including BOM-only, leading-BOM, ordinary Unicode and empty values.
It compares complete stored resource bytes and file identity/mtime, and observes no admin mutations during reads/copies; traces/screenshots are disabled and equality assertions expose only booleans.
This is selected Chromium clipboard evidence, not cross-browser behavior or clipboard-erasure assurance.
Entry-editing tests also distinguish Create, key-preserving Replace, value-preserving Rename, and deliberate empty values.
The env browser round trip checks known-collision rejection without a replacement PUT, explicit reveal, and exact material preservation through Rename without an automatic value GET.
These are known-metadata accidental-overwrite guards, not compare-and-swap or concurrent-writer guarantees.
They do not redefine backend arbitrary-byte storage or the D-Bus attribute map.

Generated-key tests distinguish transient component state from application query/mutation caches and inspect only named browser APIs: local/session storage, script-visible cookies, document and URL surfaces, Cache Storage, IndexedDB database names, and browser-context cookies.
They do not inspect IndexedDB contents, all history, HTTP cache internals, intermediaries, network/process memory, clipboard remnants, allocator state, or failure artifacts.
Synthetic lost-response routes test retained-key reconciliation, not actual network response loss.
Exact untrimmed re-entry, no-cache headers, and successful create/migration do not establish external custody, independent recoverability, or erasure in other engines.
