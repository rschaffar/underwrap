# RF-0002 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `B-01`.
Original review owners: `backend-core.md`, `backend-core-verification.md`, `backend-core-verification-lookup-commands.json`, `backend-core-verification-run-lookup.py`, `backend-core-verification-session-results.json`, `report.md`, `input-ledger.md`, `claim-evidence-join.md`, `synthesis-verification.md`, `synthesis-verification-final.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
The original documentation import did not execute or verify a correction.

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

**Disposition:** fixed in `b899a0be47090977084808a4c23b203b8d23876e`.
The unconditional `debug-embed` feature in `crates/underwrap-backend/Cargo.toml` removes request-time filesystem lookup in all backend profiles.
The selected rust-embed 8.12.0 macro emits the compiled asset table and excludes its dynamic implementation when that feature is enabled.
No dependency version or lockfile change is required.
The enduring boundary and development workflow are documented in [Security](../../../security.md#backend-transport-and-administration) and [Contributing](../../../../CONTRIBUTING.md#prepare-a-checkout).

### Executed correction checks

Checks ran on Linux x86_64 in the pinned `nix develop` shell with Rust 1.97.1 and the current Nix-built SPA supplied through `UNDERWRAP_WEB_DIST`.
The application inputs were verified again at the fixed revision; the archival delta changes finding documentation only.

| Check | Result and observed scope |
| --- | --- |
| `cargo test --locked -p underwrap-backend --lib web::tests::outside_symlink_request_returns_only_embedded_index -- --exact`, before enabling the feature | Exit 101; compilation 19.41 s; the exact-index assertion failed for the synthetic outside symlink through the real handler/router. |
| `cargo test --locked -p underwrap-backend` | Exit 0; 307 library, 3 binary, 3 admin-reference and 8 OpenAPI tests passed; no generated OpenAPI drift. Initial library execution 6.60 s. |
| `cargo test --locked --release -p underwrap-backend --lib web::tests` | Exit 0; all 8 website tests passed; initial compilation 43.05 s. |
| `cargo fmt --all -- --check` | Exit 0. |
| `cargo clippy --locked -p underwrap-backend --all-targets -- -D warnings` | Exit 0; initial run 16.16 s. |
| `nix build --no-link --print-out-paths .#checks.x86_64-linux.web-build` | Exit 0; production SPA artifact built. |
| `nix build --no-link .#checks.x86_64-linux.web-test .#packages.x86_64-linux.underwrap-backend` | Exit 0; complete SPA suite/selected coverage gate and packaged backend build passed. |
| `cargo tree --locked -p underwrap-backend -e features -i rust-embed` | Exit 0; unconditional `debug-embed` and `interpolate-folder-path` selected. |
| `cargo build --locked -p underwrap-backend` and disposable Python standard-library HTTPS probe | Exit 0; debug and exact Nix-packaged release controls passed in 0.824 s, without cookies or a client certificate. |

The retained regression creates a harmless regular file and final-component symlink outside the asset root after compilation and verifies that the symlink is absent from the asset inventory.
Literal parent components survive the Axum request construction.
GET bodies for the symlink, regular-file traversal, root, explicit index and history route equal the index, rather than merely returning status 200.
A separate test requires borrowed compile-time asset bytes even with debug assertions enabled.
Asset byte equality, MIME/cache handling and asset/fallback HEAD behavior are retained controls.
Independent read-only review found no substantive defect and confirmed feature forwarding and the macro's static lookup implementation.

### Disposable HTTPS reconstruction and limits

For each binary, create a private disposable directory, a generated localhost test certificate/key, empty registry paths and configuration with `admin: {}` and a loopback HTTPS listener.
Create only a harmless outside marker file and a sibling symlink to it after the backend is built.
Start the backend with that configuration, wait for a successful HTTPS root response within a fixed deadline, and trust only the generated test certificate for the probe.
Use Python `http.client.HTTPSConnection`, which preserves literal parent components, with no cookies or client certificate.
Request known synthetic outside-file and outside-symlink paths with enough `../` components to ascend beyond the compiled asset root; compare their exact bodies with the root index in memory.
Also verify explicit index, history and missing-asset fallback, a real hashed asset and its immutable cache/MIME headers, and empty HEAD bodies with matching headers.
Stop and reap the backend before disposing of its temporary state; print only outcomes, never bodies or private test-key contents.
Repeat for the debug binary and the exact Nix-packaged release binary.
The one-off Python probe is not retained as a repository test; the handler/router regression and static-ownership checks are retained in `crates/underwrap-backend/src/web/tests.rs`.

This establishes the corrected code and two disposable loopback HTTPS controls, not an exposure assessment or deployment of existing services.
Only synthetic files were used; no sensitive target or host symlink inventory was inspected.
Trusted build-time bundle content is intentionally public and remains outside this runtime-lookup correction.
