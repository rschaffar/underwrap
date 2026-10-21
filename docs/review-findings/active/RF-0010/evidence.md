# RF-0010 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `W-05`.
Original review owners: `web.md`, `report.md`, `finding-index.md`, `input-ledger.md`, `claim-evidence-join.md`, `substantive-inputs.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Reviewed baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The decisive chain is `web/src/pages/secrets/dialogs/create-resource.tsx:54-61,98-103` → `web/src/hooks/use-form-dialog.ts:74-100` → `web/src/pages/secrets/page.tsx:325-329`.
`web/src/components/admin-dialog.tsx:18-24,39-52` makes the optional non-dismissal contract explicit and requires footer actions to respect pending independently.

## Recorded observations

The specialist read the hook, caller and parent; the final synthesis retains independent source/control-flow confirmation.
No new representative slow-submit/dismiss/reopen interaction was executed.
The inspected `web/src/pages/secrets/dialogs/write.test.tsx:346-444` covers guarded pending/dismissal and retry in a different caller, providing contrary evidence against an application-wide claim of absent guards.
Shared-hook tests cover validation, request, invalidation and callback ownership within an instance.

The coordinator later reported 318 passing Vitest tests across 38 files and 44 configured Playwright tests.
Those suite results are separate from the specialist's source inspection and do not supply the missing focused interaction.

## Reproduction or verification procedure

The finding was established by source inspection, not execution of the following interaction:

1. Trace all close paths from Create resource and confirm which are disabled during the hook's complete pending interval.
2. Follow the awaited mutation/invalidation continuation to the parent's boolean close setter.
3. For future authorized verification, defer a harmless mocked request, dismiss and reopen the dialog, then complete the old request and refresh while the new draft is present.
4. Observe dialog-instance identity, request count and draft preservation without recording field material.
   Compare the guarded flat-entry caller.

## Limits and missing evidence

No exact interaction timing, failure capture or fixed-code result exists.
The source-identified additional callers need their own appropriate checks.
Custom resource/pair DELETE invalidation lifetime and same-turn generated-key actions were separate unpromoted leads; they are not silently included as confirmed W-05 cases.
No remote rollback, cancellation or cross-store authorization claim follows.

## Resolution evidence

### Implemented correction and source audit

Implementation revision: `c4aa251eda7dfcfaf254c73e6d0be4b255c74806`, based on current local `main` at `7260be4feff5c2bc70d958f50e50adf218cad994`.
Only RF-0010 implementation, tests and topic documentation changed; the other sessions' areas and RF-0009 decoder/reveal files remain unchanged.
The dedicated feature worktree and branch are retained without integration or remote publication.

All eleven listed callers now pass `nonDismissible={pending}` and disable explicit Cancel.
The existing `AdminDialog` supplies disabled close, Escape/outside suppression, router blocking and best-effort unload handling.
The hook still spans validation, request, invalidation and callback; only invocation of the final success callback is conditional on the originating instance remaining mounted.
Original-address invalidation still runs and can reject after unmount.

Source review found authoritative-store fallback resets in Secrets and Wrapper permissions, store-row removal in Stores, and libsecret tree error returns can unmount forms despite navigation guards.
Actual-page regressions cover resource create/rename and pair-create fallback, store edit/unlock removal, and representative collection-create refresh-error recovery during both request and invalidation.
User form parents retain dialogs outside inventory loading/error branches.
Flat editing, client enrollment/editing, permission editing and key-custody guards were inspected and left unchanged.
An independent read-only review found no additional concrete implementation regression and identified the now-covered post-unmount invalidation-rejection case.

### Executed checks

Environment: non-root Linux x86_64, pinned `nix develop`, Node 24.20.0/npm 11.19.0 and the configured Nix Playwright browsers.
Browser prerequisites were the production SPA and a backend rebuilt from this feature tree with `CARGO_TARGET_DIR="$PWD/target/browser-tests"` and `UNDERWRAP_WEB_DIST="$PWD/web/dist"`.
Browser commands selected that binary through `UNDERWRAP_BACKENDD_BINARY`.
Generated artifacts and diagnostics remain local and uncommitted.

| Check | Observed result |
| --- | --- |
| Baseline `npm --prefix web ci && npm --prefix web test` before implementation | Exit 0; 353 tests across 38 files, Vitest duration 11.84 s. |
| Negative control restoring Create resource's old dismissal behavior and unconditional hook completion; focused `form-lifecycle.test.tsx -t 'resource create'` | Exit 1; pending Cancel was enabled and replacement drafts were removed. This restores the decisive old behavior, not every byte of the old tree. |
| Negative control removing only the hook's mounted-instance condition; `form-lifecycle.test.tsx -t 'fallback-store draft'` | Exit 1; all six resource-create/resource-rename/pair-create request/invalidation cases observed the replacement dialog removed from the document. |
| Final `npm --prefix web test` | Exit 0; 378 tests across 39 files, duration 15.22 s. |
| Final `npm --prefix web run test:coverage` | Exit 0; 378 tests, duration 16.31 s; selected statements 97%, branches 84.28%, functions 98.59%, lines 98.9%. No thresholds or exclusions changed. |
| Final `npm --prefix web run build` and same-tree `cargo build --locked -p underwrap-backend` with the explicit SPA/target variables above | Exit 0; TypeScript and production build passed; backend build completed in 9.45 s. No Rust sources changed. |
| Final `(cd web && npm run test:e2e-support && npx --no-install playwright test)` | Exit 0; 9 fixture-support tests and all 47 configured browser tests, Playwright duration 33.7 s. |
| `nix build --no-link .#checks.x86_64-linux.web-test` before the final formatting-only hook change | Exit 0; 378 tests across 39 files with selected coverage, duration 21.65 s. Derivation `yahv6jbkj9jmv7jxpyalgann9vwp22d2-underwrap-web-test-0.1.0.drv`. |
| Final `nix build --no-link .#checks.x86_64-linux.web-test .#checks.x86_64-linux.web-build`, run alongside the final browser suite | Exit 1 in `web-test`; 376 passed and two unchanged tests timed out at 5000 ms, duration 37.02 s. `web-build` completed and its output was verified separately. |
| Changed-document local link/path validation and `git diff --check` | Exit 0; filesystem/link-target and whitespace checks, not browser link verification. |

`web/src/pages/form-lifecycle.test.tsx` supplies 24 StrictMode dialog/page cases with controlled request and invalidation gates.
They exercise all eleven caller controls, attempted reopen, normal completion, idle cancellation/navigation, inline request rejection and retry, and later-draft preservation.
The hook regression separately observes invalidation rejection after unmount through the retained submission promise, without global suppression.
Existing hook tests preserve observable mapping, invalidation and callback failures and full callback pending lifetime.
The complete suites retain RF-0003 identifier/creation regressions and already-guarded caller controls.

`web/e2e/form-lifecycle.spec.ts` supplies two desktop Chromium scenarios.
One delays an actual resource-create request and its active secrets-view refresh, checks disabled Cancel/close, Escape, real outside clicks, programmatic activation of the page's reopen/navigation controls, and unload-event prevention through both phases, then observes normal completion and a later editable draft.
The other checks ordinary Cancel/close/Escape/outside dismissal, an intercepted structured 503 kept inline, retained input, and successful real-backend retry.
Background-link activation deliberately bypasses modal pointer shielding to test the router lock itself.

### Failures, blocker and limits

Final pinned coverage derivation: `np56sj8qwfcnpjskgdgmahycqqijnq0n-underwrap-web-test-0.1.0.drv`.
The failures are `StoresPage > generates, exactly confirms, and creates encrypted stores without caching the key` and `RevealSecretView > reveals and copies exact Unicode, then releases cached query data`, each a 5000-ms timeout.
Those test files, encrypted-store creation and reveal implementations were not edited here.
The local baseline and final suites pass, and a prior changed-tree Nix run passes, but neither establishes the cause of the final failure or resolves RF-0012's distinct recorded aggregate failures.
Concurrent browser/build load is an observed execution condition, not a proven explanation.
No retry-to-green, timeout increase, gate weakening or out-of-scope correction follows from this evidence.
RF-0010 remains active and blocked on settling pinned SPA acceptance rather than being archived as fixed.

Initial test-development failures came from a nonexistent overlay selector and unsupported Testing Library option; browser-test setup also needed explicit Secrets navigation after the setup redirect and hidden-background-control selection while a modal is open.
Those corrections changed test code only.
An earlier combined local check reached its harness time limit despite complete pass/build output; final explicitly exited runs above replace it as acceptance evidence, not as an explanation of that timeout.
`npm ci` incidentally reported two high dependency advisories; no dependency remediation or reachability assessment is included.

The request-error route is simulated; successful resource mutations and refreshes use the disposable real HTTPS backend.
The browser unload witness dispatches a cancelable event, not forced browser closure or proof that every engine displays a prompt.
Forced parent resets are React page tests with mocked API/query state, not cross-browser or cross-tab backend concurrency witnesses.
Unmounting does not cancel submitted requests, roll back backend effects, stop an already-running callback, or promise exactly-once completion.
