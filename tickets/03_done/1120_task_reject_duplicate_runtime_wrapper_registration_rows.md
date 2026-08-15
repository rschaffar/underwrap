# Reject duplicate runtime wrapper registration rows

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-013
Source themes: C-004

## Outcome

An authenticated runtime client that repeats the same exact `(wrapper_name, resource_name)` row within one `POST /v1/wrappers` request receives HTTP 400 with stable error code `duplicate_wrapper_resource_pair`. The backend rejects the request before any `StorageBackend` read or write, does not create or alter resource, pair, or permission documents, does not panic or abort the HTTP exchange, and remains available for a later valid request.

Duplicate rows are invalid only within one request. A later request may repeat a previously accepted single row and retains the documented idempotent `outcome: ok` response with empty creation lists. Distinct rows that share only a wrapper name or only a resource name remain valid.

This corrects parser-untrusted runtime-input handling without changing runtime identity, store selection, ordinary auto-creation, permission policy, or successful registration behavior.

## Scope

- Validate the complete `RuntimeWrapperRegisterRequest` before reading the caller's auto-create flag or touching its mapped `StorageBackend`:
  - preserve the existing empty/whitespace-only `wrapper_name` and `resource_name` errors and validate those name conditions before duplicate classification;
  - classify exact Rust string equality on the pair `(wrapper_name, resource_name)`; do not trim, normalize, case-fold, or otherwise broaden name semantics;
  - reject the second occurrence of an in-request pair with HTTP 400, code `duplicate_wrapper_resource_pair`, and a generic value-free message.
- Apply duplicate rejection regardless of current storage state and `auto_create_wrapper_assets`:
  - auto-create enabled or disabled;
  - resource/pair missing or already present;
  - duplicate rows mixed with other valid rows.
- Remove the request-reachable missing-pair insertion panic. Pair-document insertion in auto-create must use checked typed propagation rather than `.expect("missing-set guarantees no duplicate")`; an unexpected internal invariant failure must become a structured error response, not unwind a request task. The stable duplicate-input 400 comes from pre-effect validation rather than from a partially applied auto-create path.
- Preserve valid behavior:
  - an empty wrapper list remains a successful no-op;
  - distinct pairs are processed in current order and retain current creation/permission results;
  - two distinct rows may share one wrapper or one resource;
  - repeating one accepted row in a later request remains successful and idempotent;
  - `auto-create-disabled`, existing empty-name codes, creator metadata, generated ids, and `Locked` permission seeding remain unchanged for valid input.
- Add deterministic production-path evidence:
  - unit validation covers exact duplicates, name-error precedence, near-miss distinct pairs, and no backing-store operation;
  - the internal auto-create seam proves a malformed duplicate missing-pair diff returns an error rather than panicking;
  - a real client-certificate-authenticated HTTPS request proves HTTP 400 and exact `ServiceErrorResponse.code`, byte-for-byte unchanged resource/pair/policy views, no connection-level EOF in place of a response, and a later valid request through the same client;
  - cover both auto-create settings and both missing and already-present pair states without turning separate-request idempotence into an error.
- Keep test servers, clients, daemon/backend processes, temporary stores, and helpers bounded and deterministically stopped on every path. Do not use sleeps as the correctness assertion.
- Synchronize current-state behavior in `docs/backend.md`, `docs/security.md`, and `docs/testing.md`: runtime mTLS authenticates the principal/store but does not make JSON input trusted; exact in-request duplicates receive the stable pre-storage 400; separate calls remain idempotent; real mTLS route coverage guards against panic/connection abort.
- Preserve these clean-HEAD revalidation artifacts outside the repository:
  - desired real-HTTPS red-before patch `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1120-red-before.patch`, SHA-256 `727bb9ae81ab59ee59182c3de203f8430516f74f0ec111d442846bf31b9dc0e1`;
  - desired red-before log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1120-red-before.log`, SHA-256 `9fa9a6a2df665034d0dae3b2417f453639caa4f00e8be5a856931f229226dc3f`;
  - current focused baseline log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1120-baseline.log`, SHA-256 `756aabb308b89c869d68de0e9ae06967c445dc70324fdeec7ec64beeb842e516`.
  At clean HEAD `2796944cbe32590e7dfca5a6981fa6960585d3c7`, the desired test fails with status 101: `apply_auto_create` panics at `runtime_api/wrappers.rs:136` with `Duplicate`, and the real mTLS client receives a TLS `UnexpectedEof` instead of a structured HTTP response. Current nine focused wrapper-registration unit tests and 25 real-HTTPS runtime API tests otherwise pass. Source ordering confirms this exact duplicate path panics while mutating local document candidates before any save call; this task does not repeat the report's broader partial-write consequence as though it were reproduced.

Expected implementation paths are limited to:

- `crates/underwrap-backend/src/runtime_api/wrappers.rs`
- `crates/underwrap-backend/src/runtime_api/wrappers/tests.rs`
- `crates/integration-tests/src/tests/runtime_api.rs`
- `docs/backend.md`
- `docs/security.md`
- `docs/testing.md`

A narrower path set is preferred if it meets every acceptance criterion. Any approved behavior-preserving cleanup has its own separately reviewed scope and commit before the finding fix.

## Non-goals

- Do not silently deduplicate or accept repeated rows in one request; the selected contract is explicit stable rejection.
- Do not reject rows merely because they share one wrapper name or one resource name, and do not reject a single unchanged row repeated in a later request.
- Do not trim, normalize, case-fold, constrain general name grammar, cap row count beyond the existing request-body bound, or change malformed-JSON/body-limit handling.
- Do not change mTLS authentication, certificate fingerprint identity, client activation, principal-to-store mapping, store-lock behavior, HTTPS/TLS policy, request extractors, or the three-route runtime surface.
- Do not change runtime request/response DTOs, serialization tags, public protocol types, admin API/OpenAPI/SPA/generated artifacts, storage schema, or error envelopes. The new stable code uses the existing `ServiceErrorResponse` shape.
- Do not make normal multi-document registration writes transactional, add rollback/CAS/revisions/locks, change concurrent last-writer-wins behavior, or claim crash/power-loss durability. O-004 and any later coordination work remain separate.
- Do not broaden referential-integrity validation or repair missing/dangling resources, pairs, permissions, blobs, client instances, or stores; F-014 remains separate.
- Do not change registry save/live-publication failure atomicity (F-015), OpenAPI security/status semantics (F-016), daemon request serialization/timeouts (F-017), privileged Nix input typing (F-031), shared-crate feature/dependency direction (F-033), JCS timestamps (F-038), dead ABI/config cleanup (F-043), or forced XDG precedence (F-044).
- Do not absorb another finding, Opportunity, unresolved item, dependency, NixOS module/test change, privileged-chain change, binary/service/thread, or severity transition.
- Do not claim general denial-of-service resistance. This task removes one deterministic authenticated-input panic and preserves the project's broader DoS non-goal.

## Acceptance criteria

### Stable pre-effect rejection

- The complete request is name-validated and checked for exact duplicate `(wrapper_name, resource_name)` rows before any backing-store read/write. Unit operation tracing proves zero `StorageBackend` operations for duplicate input.
- Every in-request duplicate returns HTTP 400 with `ServiceErrorResponse.code == "duplicate_wrapper_resource_pair"` for auto-create enabled/disabled and missing/existing pair states.
- Existing `invalid_wrapper_name` / `invalid_resource_name` behavior takes precedence over duplicate classification when a request contains an invalid name; no name normalization or new grammar is introduced.
- Rejection leaves resources, pairs, policy permissions, creator metadata, and client-instance settings unchanged. A request containing valid rows plus a duplicate commits none of them.
- No request-controlled path reaches a pair-insertion `expect` or unwinds the request task. Unexpected auto-create insertion inconsistency is returned through the existing structured error envelope.

### Valid registration compatibility

- Empty input, one valid row, multiple distinct rows, same-wrapper/different-resource rows, and different-wrapper/same-resource rows retain current behavior.
- Re-registering a previously accepted single row in a separate request remains idempotent and returns `outcome: ok` with empty creation lists.
- Valid auto-create-disabled reporting, valid auto-creation of resources/pairs, provenance, and deduplicated `Locked` permission seeding remain covered and unchanged.

### Real boundary and documentation

- A real mTLS HTTPS regression receives the exact structured 400 rather than connection EOF, proves resource/pair/policy views unchanged, then successfully performs a valid registration through the still-running service and client.
- Current-state docs clearly distinguish invalid duplicates within one request from idempotent repeated calls and state that authenticated runtime bodies remain parser-untrusted.
- F-013 is resolved. F-014–F-017, F-031, F-033, F-038, F-043, F-044, O-004, and every other finding/Opportunity remain unchanged.
- No Cargo manifest/lock, protocol DTO, OpenAPI/schema/dist, admin/SPA, Nix, privileged-chain, generated, Python-bytecode, browser-output, or source path outside the approved implementation/ticket/handover allowlists changes.

## Security boundary

- Protected asset: backend runtime-route availability and the integrity of the mapped store's resources, pairs, and permission documents.
- Attacker: an enrolled runtime client that proves possession of its registered mTLS private key but whose JSON request body remains parser-untrusted. A buggy client has the same input capability.
- Boundary direction: authenticated mTLS HTTPS body -> complete request validation -> registration diff/auto-create -> mapped `StorageBackend` control documents.
- Invariant: redundant or malformed registration input fails with a structured response before storage effects and cannot unwind the request task. Runtime authentication selects the principal and store; it does not confer trust on request structure.

## Validation

Run resource-intensive checks sequentially with `CARGO_BUILD_JOBS=2`.

- Preserve and hash the clean-HEAD red-before and baseline artifacts recorded above.
- `cargo test -p underwrap-backend runtime_api::wrappers::tests`
- `cargo test -p underwrap-backend`
- `cargo test -p integration-tests register_rejects_duplicate_rows`
- `cargo test -p integration-tests tests::runtime_api`
- `cargo test -p integration-tests`
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- Static checks for exact in-request pair uniqueness, validation-before-store ordering, absence of request-reachable registration `expect`/panic, stable error code and value-free message, unchanged valid/idempotent behavior, docs synchronization, exact staged allowlist, credential hygiene, and no generated/Python/browser artifacts.
- No VM test is required for the expected backend-only runtime-route change under the repository validation matrix. If implementation crosses into NixOS, identities, namespaces, capabilities, mounts, peer authentication, or the privileged wrapper chain, stop because that is outside approved scope rather than expanding validation after the fact.
- `nix flake check`; preserve the exact output if the known clean-baseline LLVM 21 `llvm-profdata merge` SIGILL recurs after tests pass, and do not attribute that baseline failure to F-013.

## Dependencies

- Completed F-004/F-005 establish the HTTPS-only and selected-CA mTLS transport used by the real-route regression; this task preserves those contracts.
- Completed F-011/F-012 remain binding but are not modified by this backend registration correction.
- F-014–F-017, F-031, F-033, F-038, F-043, F-044, O-004, and all other non-goals remain separately queued.
- No unresolved implementation dependency. Mandatory repository-wide and ticket-local behavior-preserving cleanup discovery begins only after this exact task is approved. No cleanup unit is pre-approved.
