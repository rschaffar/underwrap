# RF-0036 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `C-02`.
Original review owners: `documentation-replacement.md`, `documentation-coverage.json`, `backend-core.md`, `substantive-inputs.md`, `input-ledger.md`, `claim-evidence-join.md`, `synthesis-verification.md`, `synthesis-verification-final.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Owner: `docs/security.md:135`; application: `crates/underwrap-backend/src/admin_api/csrf.rs:53-83,103-155`, `crates/underwrap-backend/src/admin_api/mod.rs:105-147`; dependency selection: `Cargo.lock:390-393`.
The documentation reviewer read axum_csrf 0.11.0 src/token.rs:1-126, src/service.rs:1-32, src/cookies.rs:1-80 and src/config.rs:1-85,236-270.
These are versioned dependency-relative source locations, not repository paths or local-cache requirements.

## Recorded observations

Direct application/dependency inspection established cookie/HMAC verification without session-store access.
`crates/underwrap-backend/src/admin_api/csrf/tests.rs:1-70` covers configuration and live-principal behavior, not server-session token binding or cryptographic conformance.

The original backend matrix also recorded the six-hour default.
Independent synthesis first rejected incomplete accounting of that distinct comment mismatch, then accepted the corrected disposition after checking `crates/underwrap-backend/src/runtime_state.rs:70-94`, `crates/underwrap-backend/src/admin_api/csrf.rs:48-87` and the dependency default.
No source or runtime lifetime was changed.

## Reproduction or verification procedure

The recorded method was source inspection, not execution:

1. Compare Security's mechanism name with the application's cookie/header configuration and layer assembly.
2. Resolve axum_csrf from the repository lock and inspect the versioned token, cookie and service code.
   Trace header verification and determine whether authentication-session storage participates.
   No cookie/token values are needed.
3. Separately compare the runtime-state lifetime comment with construction and the dependency default; retain that as a contextual mismatch.
4. Review proposed wording for those exact mechanisms and limitations.
   Only a separately chosen runtime change would justify additional behavioral tests.

## Limits and missing evidence

No browser/HTTP attack test, cryptographic audit, session-binding exploit or CSRF bypass was run or established.
This record preserves two distinct documentation observations without conflating them or changing cookie lifetimes.
No new source/dependency inspection or product execution occurred during migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
