# RF-0002 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `B-01`.
Original review owners: `backend-core.md`, `backend-core-verification.md`, `backend-core-verification-lookup-commands.json`, `backend-core-verification-run-lookup.py`, `backend-core-verification-session-results.json`, `report.md`, `input-ledger.md`, `claim-evidence-join.md`, `synthesis-verification.md`, `synthesis-verification-final.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Citations describe baseline `01214454aee850ebe8ceef5b757b9ae61cca07aa`, not a new source review.
Relevant repository locations are `crates/underwrap-backend/src/web.rs:14-23,57-72`, `crates/underwrap-backend/src/http/mod.rs:63-89`, `crates/underwrap-backend/Cargo.toml:40`, and `Cargo.lock:2029-2058`.
The dependency-specific decision is in rust-embed-impl 8.12.0, src/lib.rs lines 51-57, 210-234 and 295-314; these are dependency-relative locations, not repository files.
Prior verification authenticated selected dependency identities/checksums against the workspace lock.

## Recorded observations

The independent witness imported unchanged production `web.rs` and called it through an in-process Axum fallback, preserving the literal parent component.
Debug and release runs both exited 0, taking 19.484 and 22.332 seconds including compilation.

| Request/control | Debug | Release |
|---|---|---|
| GET `/` and `/history/route` | 200, exact synthetic index, shell headers | Same |
| GET `/../outside.txt` | 200, index; no outside marker | Same |
| GET `/../outside-link` | 200, exact outside marker, non-shell headers | 200, index; no outside marker |
| HEAD `/../outside-link` | 200, empty body | 200, empty body |
| Outside symlink in asset inventory | False | False |

A separate unauthenticated HTTPS control against the exact packaged release backend returned its embedded index for both known synthetic outside paths, without the outside marker.
The release binary's recorded SHA-256 was `45da73d88ad2825bb176ad03ea42d814cf8fb1e33ce025b15f7b1e6de0e30a75`.

## Reproduction or verification procedure

Normalized reconstruction methodology, not a command run during migration:

1. Prepare a disposable external Rust harness using the locked dependencies and import the unchanged repository handler.
   Provide a synthetic asset directory containing an index, plus a sibling marker file and sibling symlink to that marker.
   Use no real private files.
2. Set `UNDERWRAP_WEB_DIST` to the synthetic assets and use external build output.
   Register the handler as an Axum fallback.
   Verify literal parent components survive URI construction and that the symlink is absent from the asset inventory.
3. Run debug and release variants with the same pinned dependencies.
   Compare exact response bytes in memory for every table row; print only statuses/equality flags.
4. For a packaged HTTPS control, start only a disposable loopback release backend and request the known synthetic paths with normalization disabled.
   Compare with its index without logging bodies.
   Stop and reap the process.

## Limits and missing evidence

This replaces the original primitive-only evidence with a real unchanged-handler/router witness, not a full debug HTTPS deployment.
Existing `crates/underwrap-backend/src/web/tests.rs:85-94` covered only regular-file traversal.
The original report's claimed historical command ledger was absent and remains unavailable; it was not reconstructed.
Fresh verification supplied the consequential commands/results.
No new reproduction or exposure assessment occurred during migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
