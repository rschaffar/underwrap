# RF-0004 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `B-02`.
Original review owners: `backend-core.md`, `backend-core-verification.md`, `backend-core-verification-session-results.json`, `substantive-inputs.md`, `report.md`, `input-ledger.md`, `claim-evidence-join.md`, `synthesis-verification.md`, `synthesis-verification-final.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Repository citations: `crates/underwrap-backend/src/admin_api/mod.rs:106-118`, `crates/underwrap-backend/src/auth.rs:38-41`, `crates/underwrap-backend/src/runtime_state.rs:82-85`, and `docs/configuration.md:207`.
Prior locked-dependency inspection covered tower-sessions 0.14.0 src/service.rs:126-142,251-280,418-454; tower-sessions-core 0.14.0 src/session.rs:289-293,664-683; axum-login 0.18.0 src/session.rs:146-183; and tower-sessions-memory-store 0.14.0 src/lib.rs:40-58.
These are dependency-relative locations.

## Recorded observations

The independent verifier started the exact release backend on an ephemeral loopback port with a four-second lifetime, disposable operator/state and freshly generated PKI.
TLS used normal hostname/IP verification against the generated certificate.
The release binary hash was `45da73d88ad2825bb176ad03ea42d814cf8fb1e33ce025b15f7b1e6de0e30a75`.

Protected GET `/v1/admin/stores` returned 200 at 0.000, 1.001, 2.005 and 3.000 seconds after login, with no session-cookie renewal.
At 4.501 seconds it returned 401/`unauthorized` and cookie removal, not renewal.
The last successful activity was only 1.500 seconds earlier.
Every request explicitly sent the original cookie, including after Max-Age.
Fresh login immediately restored GET 200.
Login confirmed Max-Age 4, Secure, HttpOnly, SameSite Strict, path `/` and absent Domain.

The harness exited 0 in 5.128 seconds.
The backend child was deliberately terminated with SIGTERM and reaped (return -15); disposable state was removed.
No credential, token, cookie or response-body content was logged.

## Reproduction or verification procedure

Normalized future methodology, not executed during migration:

1. Prepare a current release backend with its real SPA prerequisite, fresh disposable operator/registries and loopback-only HTTPS certificate; set lifetime to four seconds and configure the expected Origin.
   Never reuse production state.
2. Log in through `/v1/admin/auth/login`.
   Keep session/CSRF material in memory only and record cookie attributes, not values.
3. Explicitly replay the same session cookie on protected GETs approximately every second through three seconds, then at about 4.5 seconds.
   Do not rely on a cookie jar that silently drops expired cookies.
   Record monotonic times, statuses and renewal presence.
4. Fresh-login and protected-read controls distinguish session expiry from backend/account failure.
   For a future fix, add a separate genuine-idle interval and revocation controls.
   Always terminate/reap the child and remove disposable state.

## Limits and missing evidence

The real HTTPS witness supersedes the original source-only timing limitation.
It covers a selected protected route, not every route or browser prefix enforcement.
Genuine-idle timeout was not separately timed, and no fresh logout/restart/password-rotation run was part of this witness.
The original backend historical command ledger remains missing; fresh verification has its own recorded commands.
This migration ran no session experiment.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
