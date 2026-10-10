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

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
