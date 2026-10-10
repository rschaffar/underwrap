# RF-0009 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `W-03`.
Original review owners: `web.md`, `web-probes.mjs`, `web-probes.log`, `web-command-results.json`, `verify-W03-BOM.log`, `targeted-verification-results.json`, `report.md`, `finding-index.md`, `claim-evidence-join.md`, `substantive-inputs.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

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

## Limits and missing evidence

No clipboard wire witness or complete browser/backend round trip was recorded.
Node 22 was the specialist probe runtime, not the declared Node 24 baseline.
The separate coordinator BOM result supplies outcome/length metadata but its original invocation, exit and duration were not recovered in the read command ledger; the specialist's timing must not be assigned to it.
Prior broad SPA/browser passes do not cover the missing prefix case.
No new test was run during this migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
