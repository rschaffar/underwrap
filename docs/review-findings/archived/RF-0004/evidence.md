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

Fixed revision: `4b81f8756acae36028de0d70968065749c93c6a6`, based on local `main` at `c7c3c3d771fedffb1755942e55a219e6c1afb827`.
Correction verification used the pinned Nix development environment on x86_64 Linux, Rust/Cargo 1.97.1, the real built SPA and disposable synthetic operators.
No dependency manifests, lockfiles, OpenAPI bytes or RF-0003 implementation changed.

### Correction and negative controls

Reinspection confirmed tower-sessions/core/MemoryStore 0.14.0 and axum-login 0.18.0.
`with_always_save(true)` saves nonempty sessions on non-5xx responses, updates stored expiry and reissues the cookie without cycling its ID.
The router's Origin gate remains outside that layer.
The private `AdminSessionStore` serializes liveness-check/save with deletion; existing-session save does not create missing or expired records.
Its explicit boxed futures implement the pinned SessionStore ABI without another dependency.

The new HTTP-middleware renewal regression failed before the fix with `session renewal missing` (Cargo exit 101; test 2.73 seconds).
The deterministic late-response regression failed with always-save plus unguarded MemoryStore: logout deleted the record, the paused authenticated request completed, and old-cookie replay returned 200 instead of 401 (Cargo exit 101; test 1.21 seconds).
Both regressions pass with the guarded correction.
An initial revocation fixture incorrectly expected logout 200; it was corrected to the existing handler/OpenAPI contract's 204 before final verification.
No production status or assertion was weakened.

### Executed verification

The commands below ran from the feature root in `nix develop`, with `UNDERWRAP_WEB_DIST="$PWD/web/dist"` for direct backend Cargo commands.
These checks passed against the implementation inputs committed in the fixed revision; the subsequent finding archival changes only documentation.

| Command | Result and observed scope |
| --- | --- |
| `cargo test --locked -p underwrap-backend` | Exit 0: 314 library, 3 binary, 4 admin-reference and 8 OpenAPI tests; library suite 13.24 seconds. Includes real production-router HTTP middleware, advancing store expiry, cookie attributes, idle replay and synchronized logout interleaving. |
| `cargo build --locked -p underwrap-backend` | Exit 0; backend embeds the real SPA. |
| `cargo clippy --locked --all-targets --workspace -- -D warnings` | Exit 0, warning-free. |
| `cargo fmt -- --check` | Exit 0. |
| `npm --prefix web test` | Exit 0: 38 files, 353 tests. |
| `npm --prefix web run build` | Exit 0: generated API types, TypeScript and production Vite bundle; the existing chunk-size advisory remains. |
| `npm run test:e2e-support` in `web` | Exit 0: 9 fixture-support tests. |
| `npm run test:e2e` in `web` | Exit 0: all 44 configured browser scenarios, 30.6 seconds, against the changed-tree backend selected through `UNDERWRAP_BACKENDD_BINARY`. |
| `TMPDIR=/tmp timeout 600 ./crates/integration-tests/run` | Exit 0: all 114 integration tests, 21.20 seconds, with the runner's real SPA and packaged daemon/backend prerequisites. |
| Local Markdown link/heading and three-file-layout validation; `git diff --check` | Passed for the changed documentation; repeated after archival. |

The first integration invocation placed temporary runtime sockets beneath the long worktree path and failed (20 passed, 94 failed).
A daemon diagnostic directly identified `path must be shorter than SUN_LEN`; the fixture socket never appeared.
The successful rerun changed only the runtime-fixture `TMPDIR` to `/tmp`, not source or assertions; build/browser output and logs remained worktree-local.

### Direct HTTPS and revocation witnesses

A separate standard-library HTTPS probe used the changed-tree debug backend, a disposable self-signed certificate with verified localhost/IP SANs, offline provisioning through password stdin and a four-second interval.
Six protected GETs one second apart explicitly replayed the original cookie beyond its initial deadline, all returned 200 and renewed Max-Age 4 with Secure, HttpOnly, SameSite Strict, path `/` and no Domain.
After 4.2 seconds without requests, replay returned 401; fresh login restored 200.
Stopping/reaping the backend and starting a new process over the same persisted operators/configuration rejected a fresh pre-restart cookie with 401, while post-restart login restored 200.
The probe exited 0 and terminated/reaped its backend on completion; it emitted decision metadata only.
The procedure is the earlier HTTPS methodology extended with idle and same-state process-restart controls, not dependent on a retained temporary probe.

`admin_api::tests` additionally exercises same-user login, public whoami/bootstrap reads, CSRF 403 and validation 400 renewal, Origin 403 and handler 500 nonrenewal, and anonymous requests.
It checks password change retains the submitting session while old sessions lose identity through public whoami and protected requests; reset and deletion independently deny stale sessions.
Logout removes the stored record and old-cookie replay denies; a gated request body synchronizes authentication-before-logout and response-after-logout without timing sleeps.
The Rust restart witness reconstructs fresh state over persisted users and includes a fresh-login control; the separate HTTPS probe supplies actual process-restart evidence.
Store tests separately reject missing/deleted/expired saves while retaining positive create/live-save behavior.
Independent read-only review of the pinned dependency flow, guard, wiring and tests found no remaining correctness issue; that review executed no checks.

### Remaining limits

The selected routes, interleaving and configured browser projects are bounded evidence, not every concurrent schedule or general browser/security assurance.
Already-admitted work can finish and emit a stale cookie after logout, but the guarded save cannot restore its authority.
Deleted-user nonempty records can still renew, but live user resolution denies their authority.
The pinned npm install reported one high-severity advisory; dependency remediation is outside RF-0004 and no dependency selection changed.
No NixOS/privileged deployment boundary changed, and no VM run is claimed.
