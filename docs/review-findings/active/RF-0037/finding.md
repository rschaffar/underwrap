# RF-0037 — Dev-only source-map-js lock selection has a patched advisory

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

`web/package-lock.json:3754-3762` locks source-map-js 1.2.1 with `dev: true`.
The recorded advisory GHSA-68fv-2mgg-jv7q / CVE-2026-93749 concerns insufficient validation of indexed-source-map section line offsets.
Extremely large offsets can cause synchronous event-loop blocking when supplied to the vulnerable Node tooling.

The recorded affected range is >=1.0.0 and <1.2.2, with first patched version 1.2.2.
Recorded dependency explanation routes the package through PostCSS/Vite, Tailwind, jsdom/css-tree and coverage/magicast.
Reaching a build/test worker with crafted source-map input is a prerequisite; the review did not demonstrate that input path in a deployed service.

## Consequence and boundary

The bounded asset is development/build/test worker availability, not production broker confidentiality.
The upstream advisory is High, but project priority is Low maintenance given dev-only selection and the demonstrated deployment boundary.
`web/vite.config.ts:18-20` disables production sourcemaps, while `flake.nix:91-94` builds a static SPA and excludes Node from the ordinary backend runtime closure.
Neither fact proves all development inputs are trusted; both limit claims about shipped runtime exposure.

One audit advisory is not evidence of a production high-severity vulnerability or of a broad missing DoS guarantee.
No crafted-offset workload was executed, no event-loop stall measured, and no raw resource or key material was implicated.
Available unrelated package updates from npm outdated are informational, not findings or permission to change compatibility policy.

## Correction and alternatives

Refresh the existing transitive lock resolution to a compatible patched source-map-js and update the associated Nix npm dependency hash.
This need not add a new direct dependency or change major-version support policy.
Confirm parent dependency constraints and reproducibility rather than applying a broad automatic audit fix.
Any decision to broaden production parser exposure would require a separate threat-model review.

## Acceptance criteria

- Confirm every relevant locked occurrence is outside the affected range and that installed dependency/peer resolution is consistent.
- Regenerate/verify the corresponding Nix npm hash and rebuild using the supported locked toolchain.
- Run the existing SPA production build, unit/coverage and configured browser compatibility checks; record scope instead of claiming all-platform assurance.
- Recheck the exact advisory/range and retain dev/runtime classification.
  Do not treat removal of an audit warning as proof against all crafted inputs.
- These are checks for a future authorized update; no dependency or hash has been changed by this record.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
