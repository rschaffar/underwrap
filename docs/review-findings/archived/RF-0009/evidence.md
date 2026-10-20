# RF-0009 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `W-03`.
Original review owners: `web.md`, `web-probes.mjs`, `web-probes.log`, `web-command-results.json`, `verify-W03-BOM.log`, `targeted-verification-results.json`, `report.md`, `finding-index.md`, `claim-evidence-join.md`, `substantive-inputs.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
The original import itself did not execute or verify a correction; later authorized work is recorded below.

## Source evidence

Reviewed baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The decisive decoder is `web/src/lib/utf8-base64.ts:35-38`; consumers are `web/src/pages/secrets/reveal-secret-view.tsx:64-72,122-129`.
The specialist fully inspected `web/src/lib/utf8-base64.test.ts` and `web/src/pages/secrets/reveal-secret-view.test.tsx`; neither supplied a leading-BOM witness.

## Recorded observations

The specialist's external combined W-02/W-03 probe transpiled the actual helper in memory under Node v22.23.3.
It exited 0 in 0.82635 seconds with zero network requests.
Decoding succeeded, exact equality was false, and 7 input code points became 6.
The successful process exit records reproduction of the defect, not correct behavior.

A separate coordinator actual-helper witness recorded `decoderSucceeded=true`, `exactRoundTrip=false`, `inputLength=10`, and `outputLength=9`.
These are a second witness's reported length fields, not a correction of the specialist's 7-to-6 code-point count.
Neither input nor encoded material is retained here.
The final synthesis accepts the independent helper confirmation.

## Reproduction or verification procedure

Normalized method, not a fresh execution or an assertion that an external probe exists in the repository:

1. Load the current actual helper with the matching locked TypeScript/web dependencies.
2. In memory, prepend U+FEFF to harmless test text, encode with `encodeUtf8Base64`, then decode with `decodeBase64Utf8`.
3. Compare success, exact equality and lengths only; use ordinary text and empty text as controls.
4. A future UI check must separately verify reveal and clipboard equality for both caller families without persisting values or browser captures.

## Discovery limits and missing evidence

No clipboard wire witness or complete browser/backend round trip was recorded.
Node 22 was the specialist probe runtime, not the declared Node 24 baseline.
The separate coordinator BOM result supplies outcome/length metadata but its original invocation, exit and duration were not recovered in the read command ledger; the specialist's timing must not be assigned to it.
Prior broad SPA/browser passes do not cover the missing prefix case.
No new test was run during this migration.

## Resolution evidence

### Correction and environment

Fixed in `5acc9e9db66d084f02e9098cb4d261ab7e4ec360`, based on local `main` at `c7c3c3d771fedffb1755942e55a219e6c1afb827`.
The shared decoder now uses `fatal: true, ignoreBOM: true`; no caller-specific workaround, backend change or dependency change is involved.
Application, build and browser commands below ran inside the pinned `nix develop` environment on Linux, using Node 24.20.0, npm 11.19.0 and Rust 1.97.1.
Documentation and Git checks ran on the host.
Build, temporary fixture and browser output stayed within the dedicated feature worktree.
The later finding archival changes only documentation; runtime and test inputs remain those of the fixed revision.

### Executed checks

| Command | Exit and observed result | Reported duration |
| --- | --- | --- |
| `npm --prefix web ci`, then pre-fix `npm --prefix web test -- src/lib/utf8-base64.test.ts` | Install succeeded; test exited 1: BOM-only, leading-BOM Unicode and repeated-leading-BOM equality failed, with seven controls passing and only boolean assertion output | Test 1.32 s |
| `npm --prefix web test` | 0; 38 files, 359 tests passed, including shared-view BOM reveal/mock-clipboard, empty/missing and malformed/invalid controls | 13.50 s |
| `npm --prefix web run test:coverage` | 0; 359 tests passed; selected API/library statements 97%, branches 84.28%, functions 98.59%, lines 98.9%, all above the unchanged 80% gates | 16.78 s |
| `npm --prefix web run build` | 0; generated local API types, TypeScript check and production SPA build passed | Vite 2.69 s |
| `CARGO_TARGET_DIR="$PWD/target/browser-tests" UNDERWRAP_WEB_DIST="$PWD/web/dist" cargo build --locked -p underwrap-backend` | 0; same-tree backend built with the changed SPA prerequisite | 38.59 s |
| `npm --prefix web run test:e2e-support` | 0; nine browser-fixture lifecycle checks passed | 0.328 s |
| `(cd web && npm exec -- playwright test e2e/secret-text.spec.ts e2e/secrets.spec.ts --project=desktop-1280 --workers=1 --output=../target/rf-0009/browser-results-final)` | 0; eight real-backend browser flows passed, including the unchanged RF-0003 creation/identifier scenarios | 37.4 s |
| Topic-document local link/path checks and `git diff --check` | 0; three topic documents passed; finding links/layout and staged changes checked again for archival | Not measured |

The browser invocation exported `UNDERWRAP_BACKENDD_BINARY="$PWD/target/browser-tests/debug/underwrap-backend"`, a worktree-local `TMPDIR` and `PLAYWRIGHT_NO_COPY_PROMPT=1` before entering `web`.
No live resource or credential was used.
The new spec disables traces, screenshots and video, closes its page before failure snapshotting, and compares synthetic material in memory with boolean-only equality assertions.
It serializes its clipboard cases and seeds a nonempty sentinel before each copy, so an empty result cannot pass merely because the clipboard started empty.

Two earlier browser invocations exited 1 because the new test initially extracted `id` instead of `resource.id` from resource creation (30.4 s), then selected both dialog Close buttons (38.5 s).
These failures occurred in test setup/selection, not decoder equality assertions; the response extraction and selector were corrected before the successful final run.
Existing Secrets flows passed in both earlier runs.
Failed-run artifacts were retained locally, not published.

### Acceptance observations and limits

`web/src/lib/utf8-base64.test.ts` covers BOM-only, leading/repeated/interior BOMs, ordinary Unicode, empty text, multi-chunk encoding, malformed base64, invalid bytes, a truncated BOM and a valid BOM followed by invalid bytes.
`web/src/pages/secrets/reveal-secret-view.test.tsx` separately verifies exact BOM reveal and clipboard arguments using a mock; that is not real clipboard evidence.
`web/e2e/secret-text.spec.ts` exercises the actual shared dialog callers for env, files and libsecret with BOM-only, leading-BOM Unicode, ordinary Unicode and empty values.
It presses Show and Copy, reads the real Chromium clipboard, and checks exact revealed and copied code points.
After each read/copy it compares the complete plain-store resource buffer with the pre-read snapshot, verifies unchanged file identity/mtime, and observes no admin mutation requests.
This directly supports unchanged storage for the exercised flows, not all storage backends, concurrent writers, universal browser support or clipboard erasure.

The install reported one high-severity advisory, not investigated or changed in this RF-0009-only task.
Vitest emitted existing jsdom `scrollTo` warnings, and Vite reported a large-chunk warning; neither failed its check.
No required RF-0009 acceptance check remains unavailable.
Initial completion retained the feature commits, branch, worktree and build/browser artifacts without integration or publication.

### Rebased integration candidate

The user subsequently requested local rebase and merge.
Rebase onto local `main` at `7260be4feff5c2bc70d958f50e50adf218cad994` completed without conflicts.
The implementation is now `3cda9e60a13eed508bc9fe0eb3dd73eb0f2cf80b`; the tested rebased candidate is `ddd6e304da74800fd4ded7acdff139315dd6379f`.
Range comparison confirms the implementation patch is unchanged; the archive commit only incorporates the current register context.

The same pinned-environment commands listed above were rerun on this candidate: full Vitest and selected coverage each passed all 359 tests, coverage remained 97% statements / 84.28% branches / 98.59% functions / 98.9% lines, and production SPA and same-tree backend builds passed.
All nine browser-fixture support checks and all eight relevant real-backend Chromium flows passed again; the browser run took 27.4 seconds.
Its output directory was `target/rf-0009/rebased/browser-results`, with the same binary, temporary-directory and no-capture settings.
The following evidence-only documentation update leaves all tested application and test inputs unchanged and is checked for links, paths and whitespace before integration.
No cleanup or remote operation is part of the local integration request.
