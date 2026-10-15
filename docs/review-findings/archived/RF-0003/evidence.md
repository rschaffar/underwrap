# RF-0003 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `F-01`, `A-01`, `S-02`, `W-01`.
Original review owners: `formats-storage.md`, `backend-data.md`, `secret-service.md`, `web.md`, `substantive-inputs.md`, `coordinator-notes.md`, `verify_admin_publication.ts`, `verify_formats.rs`, `verify-F01-admin-publication-origin-corrected.log`, `targeted-verification-results.json`, `input-ledger.md`, `claim-evidence-join.md`, `synthesis-verification.md`, `synthesis-verification-final.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
The original import did not execute or verify a correction.
Its Medium disposition consolidated A-01, S-02 and W-01; W-01's original High proposal was narrowed to workflow correctness without reducing confidence.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Besides the producer/validator/UI locations above, the merged trace cites `crates/underwrap-backend/src/admin_api/libsecret.rs:121-141`, `crates/underwrap-backend-store/src/resource_blob.rs:33-58`, `crates/underwrap-backend/src/runtime_api/invocations.rs:124-140`, `crates/underwrapd/src/backend_client.rs:270-285`, `crates/underwrapd/src/wrapper_flow/read.rs:98-139,183-193`, `crates/underwrapd/src/wrapper_resource_channel.rs:146-165`, and `crates/underwrap-secretsd/src/section_store.rs:19-80,84-118`.

Inspected tests intentionally accepted the broader admin grammar/default (`crates/underwrap-backend/src/admin_api/libsecret/tests.rs:16-57`) while protocol tests rejected incompatible IDs (`crates/underwrap-protocol/src/resource_blob/tests.rs:459-538`).
Integration CRUD at `crates/integration-tests/src/tests/admin_libsecret.rs:181-247` did not project its created resource.

## Recorded observations

The coordinator's corrected disposable HTTPS verifier received 200 for resource creation, collection creation and omitted-ID item creation.
The generated item failed the 32-lowercase-hex shape check.
A compiled current Rust validator rejected a synthetic section using the actual returned identifiers; replacing only the item ID with a valid 32-hex control made validation succeed.
HTTP accepted a dotted collection ID with status 200, but that identifier with a valid item ID failed the same validator.

The first verifier run exited 1 after 5.21 seconds because its request omitted the configured Origin (HTTP 403).
Correcting the external verifier produced exit 0 in 3.03 seconds; no product fix was made.
The historical HTTP fixture used disposable TLS with client certificate-verification bypass, so this is not TLS-authenticity evidence.
Only empty synthetic material, identifiers and outcome booleans were used.

## Reproduction or verification procedure

Normalized reconstruction, not executed during migration:

1. Prepare a disposable loopback backend with real SPA, writable default store, fresh operator and test TLS.
   The repository's `web/e2e/backend.ts` supplies a backend fixture; do not reuse live state or publish its artifacts.
   Authenticate via `/v1/admin/auth/login`, retaining cookies and CSRF in memory and sending the configured Origin.
2. POST a resource to `/v1/admin/stores/default/resources`.
   Under its returned ID, POST `/libsecret/collections` with no collection ID, then `/libsecret/collections/{collection_id}/items` with the item ID omitted and empty synthetic secret material.
   Record statuses and IDs only.
3. Using the current protocol crate, construct a minimal typed LibsecretSection containing those IDs and empty material; serialize it, then call `LibsecretSection::from_strict_publication_bytes`.
   Repeat with a valid 32-lowercase-hex item ID and with an HTTP-accepted dotted collection ID.
   Keep bodies out of logs.
4. Separately add a real authorized libsecret-wrapper startup/readiness test for closure.
   The recorded verifier did not perform this step or load a complete persisted section into a running namespace.

## Limits and missing evidence

HTTPS producer plus actual strict validator supersedes reviewer-time source-only uncertainty, but conversion, full startup and alias-punctuation consequences remain source-traced.
The recorded positive control establishes identifier compatibility, not universal projectability.
No new source inspection, fixture execution or wrapper invocation occurred during migration.

## Resolution evidence

Fixed revision: `83a19ca2b62fbb8ccdc131fc30d365a8ffce634f`, based on local `main` at `8f43fc81202aff2cc6ba6069a5340d42ba12448e`.
Checks used the pinned x86_64-linux Nix development/build environment and disposable fixtures, not live brokered resources.

### Regression and acceptance

- Before the production fix, `cargo test --locked -p underwrap-backend --lib default_admin_item_survives_stored_state_strict_publication` exited 101 at its publication-acceptance assertion after successful default creation and stored-state loading.
  The final expanded backend libsecret suite passes all nine tests, covering omitted/explicit IDs, case/length/nonhex and punctuation bounds, permitted underscores, whole-resource rejection equality, duplicate protection and incremental legacy inspection/deletion.
- `nix build --no-link .#checks.x86_64-linux.vm-test .#checks.x86_64-linux.workspace-test .#checks.x86_64-linux.workspace-clippy .#checks.x86_64-linux.web-test` exited 0 on the final application/test inputs.
  Workspace tests include strict publication, namespace and Secret Service regressions; Clippy denies warnings, and the SPA coverage gate passed all 353 tests.
  The final VM script completed in 155.82 seconds.
- `nix/tests/test.py::local_libsecret_mode` creates a collection and item through authenticated HTTPS with both IDs omitted, reads persisted state, and checks invalid-identifier rejections leave it unchanged.
  The real `libsecretBusProbe` target looks up both the admin-created item and an existing item before each of its two readiness markers.
  This requires the deployed namespace's strict generation-zero acceptance and private-service readiness before target exec; final stored state remains unchanged after the lookups.
- `local_legacy_libsecret_disabled_projections` installs trusted synthetic legacy identifiers, runs env-only and files-only wrappers, and checks the complete expected stored result.
  Its files control requires a previously absent file to persist, so target exit alone cannot mask suppressed writeback.
  Original resources are restored in `finally`.
- `./crates/integration-tests/run --locked` exited 0: 114 tests passed in 20.70 seconds, including actual HTTPS creation/reveal/delete, segment admission controls and the complete-resource size boundary.
  The oversized-item control retains a valid ID and adds one label byte, preserving its intended size rejection.
- `npm --prefix web run test:coverage` and `npm --prefix web run build` exited 0.
  Coverage passed with statements 97%, branches 84.28%, functions 98.59% and lines 98.9% for the selected API/library scope.
  After building the same-tree backend with `UNDERWRAP_WEB_DIST` and setting `UNDERWRAP_BACKENDD_BINARY`, `(cd web && npm run test:e2e-support && npm run test:e2e -- secrets.spec.ts --project=desktop-1280)` exited 0: nine fixture tests and four browser flows passed; Playwright took 26.4 seconds.
  The libsecret flow rejects incompatible IDs inline, leaves the item ID blank, checks the returned ID, then reveals and deletes that item.
- `cargo check`, `cargo test` and `cargo clippy` with `--locked -p underwrap-protocol --no-default-features --features local-abi` exited 0; 64 isolated-profile tests passed, with Clippy using `-- -D warnings`.
  `cargo fmt -- --check` and `cargo test --locked -p underwrap-backend --test openapi_export --test admin_reference` also exited 0.
  The initial expected OpenAPI drift rewrote the generated contract, which was inspected and rechecked.
  An initial aggregate run identified the added generic RNG-error path in the exact internal-error inventory; the inventory was updated with an explicit context assertion before the passing final run.
- Local Markdown paths/anchors, finding layout/index links, VM Python syntax and `git diff --check` passed.
  Independent read-only review covered the resource transport/publication invariants and the implementation/tests; the files-only witness was strengthened to require persisted change before final VM acceptance.

### Remaining limits

Existing incompatible state is not automatically renamed or migrated.
Legacy read/delete admission remains permissive; RF-0008's separate dot-segment URL addressing defect is not resolved here.
RNG failure and forced random-collision branches were source-reviewed, not fault-injected.
The VM is one x86_64-linux loopback deployment and the browser witness is desktop Chromium; these results do not establish universal projectability, complete D-Bus conformance or a broader security audit.
The discovery evidence above remains historical rather than being recast as a full-startup test.
