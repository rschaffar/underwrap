# RF-0003 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `F-01`, `A-01`, `S-02`, `W-01`.
Original review owners: `formats-storage.md`, `backend-data.md`, `secret-service.md`, `web.md`, `substantive-inputs.md`, `coordinator-notes.md`, `verify_admin_publication.ts`, `verify_formats.rs`, `verify-F01-admin-publication-origin-corrected.log`, `targeted-verification-results.json`, `input-ledger.md`, `claim-evidence-join.md`, `synthesis-verification.md`, `synthesis-verification-final.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

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

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
