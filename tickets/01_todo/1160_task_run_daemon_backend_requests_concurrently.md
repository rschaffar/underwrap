# Run daemon backend requests concurrently within bounded deadlines

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-017
Source themes: C-004

## Outcome

Independent `underwrapd` runtime HTTPS calls no longer pass through one global FIFO/current-thread worker. `BackendClient` owns the existing shared async reqwest client directly; register, invocation-open, and invocation-finalize are async calls admitted fail-fast through a bounded in-flight semaphore and each admitted call has one end-to-end deadline covering network I/O, error-body handling, and response decoding. One stalled backend call therefore cannot head-of-line block an unrelated call while capacity remains, and overload cannot create a pending request queue.

This preserves the backend as policy/persistence authority and preserves every mTLS, HTTPS-only, CA-trust, endpoint, payload, error-mapping, and wrapper-facing response rule. A timed-out request is cancelled locally, but its remote outcome is unknown: especially for finalize, the backend may already have committed before the response deadline. No rollback, retry, exactly-once, transaction, or ordering claim is introduced.

## Scope

- Replace the single `std::sync::mpsc<Command>` worker and private current-thread Tokio runtime in `crates/underwrapd/src/backend_client.rs` with direct async methods over the one existing reqwest `Client` and base URL.
  - `BackendClient::open` remains a synchronous constructor that validates the URL before credential reads, loads/parses mTLS material, applies the existing configured-CA-exclusive/default-WebPKI policy, keeps `https_only(true)` and `pool_max_idle_per_host(0)`, and returns without spawning a request worker.
  - `register`, `open_invocation`, and `finalize_invocation` become async and retain their exact request DTOs, endpoint paths, content type, response DTOs, and `StoreLockedError`/`UnauthorizedError`/service-error mapping.
- Give the client a fixed, named first-release concurrency/deadline policy:
  - at most **32** in-flight backend runtime requests per daemon client;
  - admission uses `try_acquire`/`try_acquire_owned`: there is **no pending client queue**; the 33rd simultaneous call fails immediately through the existing startup/open/finalize internal-error handling;
  - one **30-second** end-to-end deadline starts immediately after successful admission and ends only after the complete success/error body is decoded;
  - timeout or caller cancellation drops the request future and its permit; no blocking task or detached request worker remains;
  - saturation and timeout errors contain operation/bound context only and never request bodies, resource blobs, credentials, keys, or response bodies.
  Private test construction may inject smaller limits/deadlines; production values remain the exact named constants above and are not new configuration. The user explicitly selected this fail-fast 32/30-second policy after D-111 stopped automatic execution for the otherwise unresolved overload decision.
- Await the async client at every call site:
  - startup best-effort registration awaits its bounded call before continuing with the existing success/warning behavior;
  - wrapper invocation open awaits without blocking a Tokio worker thread;
  - wrapper-resource `Checkin` and `UsageSignal` await finalize directly; remove the now-obsolete `spawn_blocking` bridge and per-request cloning used only to satisfy its `'static` closure;
  - one wrapper-resource channel still processes its own frames sequentially. Separate invocations/calls may complete in either order; the backend remains authoritative and current last-writer-wins/concurrency limitations do not become transactions.
- Add deterministic test-only TLS fixture coverage, preferably in a sibling `backend_client/tests/concurrency.rs` module so test files stay bounded:
  - synchronize on the backend having fully accepted/read a first invocation-open request, deliberately withhold its response, then prove a second unrelated request reaches the backend and completes before the first is released;
  - run the client side on a current-thread Tokio test runtime so a reintroduced synchronous/blocking call is observable;
  - use channels/barriers and finite socket/test deadlines rather than sleeps as primary synchronization;
  - with a test-only short request deadline, prove a deliberately unanswered accepted request returns a timeout and releases its permit;
  - with a test-only one-slot limit, prove a second simultaneous request is rejected immediately rather than queued, then prove a later request is admitted after the first releases its permit;
  - preserve exact HTTPS/TLS fixture behavior and close/reap every server/client thread/task on both success and assertion paths.
- Adapt the existing HTTPS-to-HTTP redirect test to the async API without weakening its proof that the plaintext trap receives zero connections.
- Keep or add focused call-site tests as needed to pin unchanged wrapper error translation and channel response variants; do not duplicate backend policy tests.
- Synchronize `docs/architecture.md`, `docs/security.md`, and `docs/testing.md` with direct async concurrency, exact 32/30-second bounds, timeout/cancellation/unknown-remote-outcome semantics, and the deterministic stall regression.
- Preserve clean-HEAD evidence outside the repository:
  - red-before patch `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1160-red-before.patch`, SHA-256 `dad54121b6eeb0aae1c1e37ef213b1d67a463057849fdf90b8b717be0605b897`;
  - red-before log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1160-red-before.log`, SHA-256 `713f4db36883cc44aaecaecaf175f56bbaa78b677ce07a84f432663fe15a414e`;
  - clean underwrapd baseline `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1160-baseline.log`, SHA-256 `7e6dd613cddc5e1984cf86bdeae46cd4502dcd19eb84d8b081b7fde2c7a4fcf7`.
  The synchronized red test `independent_runtime_requests_are_not_globally_serialized` exits 101 because the second unrelated request cannot reach the test backend during the first held response. The clean baseline passes 41 library, two binary, and zero doc tests.

Expected implementation/product paths are limited to:

- `crates/underwrapd/src/backend_client.rs`
- `crates/underwrapd/src/backend_client/tests.rs`
- optionally `crates/underwrapd/src/backend_client/tests/concurrency.rs`
- `crates/underwrapd/src/server.rs`
- `crates/underwrapd/src/wrapper_flow/mod.rs`
- `crates/underwrapd/src/wrapper_flow/read.rs`
- `crates/underwrapd/src/wrapper_resource_channel.rs`
- existing underwrapd sibling test modules only if call-site adaptation requires them
- existing integration harness/tests only if the unit TLS fixture cannot prove the daemon-level call path without a production hook
- `docs/architecture.md`
- `docs/security.md`
- `docs/testing.md`

A narrower product path set is preferred if it satisfies every criterion. Required board moves and `tickets/_handover/codebase_2026-07-18.md` updates are workflow state, not product-scope expansion. Mandatory fresh cleanup discovery runs first; no cleanup is pre-approved. A behavior-preserving extraction of duplicated test TLS setup qualifies only if it is a clear independent net reduction and is reviewed/validated/committed before the fix.

## Non-goals

- Do not change runtime API paths, DTOs, protocol/wire formats, backend handlers, policy evaluation, persistence, writeback validation, admin/OpenAPI/SPA behavior, or NixOS/privileged wrapper-chain behavior.
- Do not change client credential reload/caching or add unauthorized-triggered rebuild/retry; F-018 remains separate. Do not add retries, backoff, hedging, failover, circuit breakers, keep-alive reuse, connection pooling changes, or backend discovery.
- Do not add a deadline/concurrency Nix/YAML option, dependency, global runtime, thread pool, detached worker, pending request queue, per-call OS thread, or production fault hook.
- Do not claim that timeout/cancellation prevents backend effects. Do not retry finalize automatically: a request may have committed before its response was lost or timed out.
- Do not introduce cross-request ordering, per-resource locking, compare-and-swap, transactions, exactly-once delivery, rollback, or a stronger concurrent-write guarantee. Existing same-channel frame sequencing remains; separate calls are independent.
- Do not expose secret material, request/response bodies, credentials, resource blobs, or keys in timeout/errors/logs.
- Do not absorb F-018 or any later finding, Opportunity, unresolved item, migration, or severity transition.

## Acceptance criteria

### Independent bounded execution

- No `Command`, std mpsc request queue, backend request worker thread, private Tokio runtime, synchronous backend call, or `spawn_blocking` backend bridge remains.
- Register/open/finalize use the shared async reqwest client and can make progress independently up to exactly 32 in-flight calls. A 33rd simultaneous call is rejected immediately and never becomes a semaphore waiter or queued command.
- Every admitted call has one exact 30-second deadline from successful fail-fast admission through complete response/error decoding. Timeout/cancellation releases capacity.
- A deterministic synchronized regression proves an unrelated second request reaches and completes while the first response is held. Regressions prove an accepted unanswered request times out/releases capacity and one-slot saturation rejects immediately without a pending waiter before later admission succeeds.

### Preserved authority and transport

- URL validation still precedes credential reads; only absolute HTTPS-with-host is accepted; redirects cannot reach HTTP; configured CA trust remains exclusive; absent CA retains bundled roots; mTLS identity, disabled keep-alive, endpoint/base-path behavior, content type, DTOs, and typed error translation remain unchanged.
- Startup registration remains best-effort and bounded. Invocation-open/finalize wrapper-facing success, deny, store-locked, unauthorized/enrollment, invalid-blob, and internal-error behavior remains unchanged apart from no global HOL and bounded timeout.
- One channel remains sequential. No timeout response is documented as proof of non-commit, and no retry/transaction/order guarantee is added.

### Tests, docs, and scope

- The preserved red concurrency test fails for exact global serialization before the fix and equivalent green evidence passes after it, without primary sleep synchronization or leaked threads/tasks.
- Exact constants/default policy and timeout/permit-release/fail-fast saturation behavior are pinned by tests using private test-only injected bounds, not by waiting 30 production seconds.
- Existing underwrapd, integration, transport/CA/restart, open, writeback, and usage-signal tests pass. Current architecture/security/testing docs state the exact bounded-async contract and unknown remote outcome after timeout.
- No dependency/config/protocol/backend/OpenAPI/SPA/Nix/privileged-chain change, production fault hook, secret logging, generated output, path outside the approved allowlist, or adjacent F-id effect. Required ticket/handover paths remain permitted workflow state.
- F-017 is resolved. F-018+, retries/failover/tuning, stronger write ordering, and every other finding/Opportunity remain separately queued.

## Security boundary

- Protected asset: availability and bounded latency of one legitimate wrapper invocation or writeback when another daemon runtime HTTPS call is stalled or the local caller load saturates client capacity.
- Actor/fault: a slow/unresponsive trusted backend or network stalls one call; an untrusted local caller can also generate wrapper traffic that consumes bounded slots. Neither may create a pending internal request queue, and one occupied slot must not globally block unrelated calls while other capacity exists.
- Boundary direction: local wrapper/channel calls -> shared daemon `BackendClient` admission/deadline -> runtime mTLS HTTPS -> backend response.
- Invariant: policy and persistence remain backend-authoritative; transport stays HTTPS+mTLS; admission is fail-fast and bounded; each admitted call is independently deadline-bounded; cancellation is local and does not imply remote rollback.

## Validation

Run resource-intensive checks sequentially with `CARGO_BUILD_JOBS=2` and `PYTHONDONTWRITEBYTECODE=1`. Hard bounds belong in parent processes; do not place cancellation-unsafe blocking futures under in-runtime timeout wrappers.

- Re-hash the red/baseline artifacts above.
- Exact synchronized concurrency, accepted-request deadline/permit release, fail-fast saturation/later admission, redirect, CA-policy, endpoint, and credential-validation tests.
- `cargo test -p underwrapd`
- Focused integration tests covering daemon reads, runtime API/store reads, writeback/finalize/usage signal, backend CA/HTTPS transport, and backend restart.
- `cargo test -p integration-tests`
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cd web && npm test && npm run build`; remove generated `web/dist` and uncommitted schema output afterward.
- Static inventory: no old worker/mpsc/private runtime/backend `spawn_blocking` or pending waiters; exact fail-fast 32/30 constants; all three client methods and all call sites async; one absolute deadline covers each admitted request through response decode; unchanged TLS/CA/URL/pooling/error mapping; no credential reload/retry/config/dependency/protocol/backend/Nix/privileged diff; exact staged allowlist; no test processes, worktrees, generated/Python/browser artifacts.
- No standalone VM test is required for the unprivileged daemon HTTP scheduling boundary. Stop if implementation crosses NixOS, identities, namespaces, capabilities, mounts, peer authentication, or the privileged wrapper chain.
- `nix flake check`; preserve exact output if the known clean-baseline LLVM 21 `llvm-profdata merge` SIGILL recurs after tests pass.

## Dependencies

- Completed F-004/F-005 define the HTTPS-only URL/redirect and exclusive configured-CA trust rules that must remain unchanged.
- Completed F-006–F-009/F-012 define writeback, handoff, and resource-size behavior whose call-site semantics must remain unchanged.
- Completed F-015 defines registry failure behavior; completed F-016 defines generated admin contract. Neither is changed.
- F-018 credential reload/restart guidance, retries/failover/configuration, stronger write ordering, and all later findings remain separate.
- No unresolved implementation dependency. Cleanup discovery begins automatically under D-111 after materialization; no cleanup unit is pre-approved.
