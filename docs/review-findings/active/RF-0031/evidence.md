# RF-0031 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `W-06`.
Original review owners: `web.md`, `report.md`, `finding-index.md`, `substantive-inputs.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Reviewed baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Primary wiring: `web/src/pages/wrapper-permissions/dialogs/create-pair.tsx:102-177`.
Shared context and ID generation: `web/src/components/ui/form.tsx:51-123`.

## Recorded observations

The visible Resource label uses the `mode` FormItem ID.
The conditional Select/Input receives the different ID of its own FormItem, and no explicit labelled-by relationship joins them.
The radio choices have their own wrapped labels; the finding does not claim those radios are wholly unnamed.
The specialist recorded source inspection only.

## Reproduction or verification procedure

1. Read the `mode`, `resource_id` and `new_resource_name` field subtrees.
2. Follow each FormItem's generated ID through FormLabel and FormControl; compare `htmlFor` with the actual input/select ID.
3. After an authorized correction, render each mode and inspect its accessible name and label focus behavior rather than selecting solely by test ID.

Steps 1-2 describe source inspection, not a browser execution.
Step 3 remains prospective.

## Limits and missing evidence

No screen-reader, accessibility-tree, visual-contrast or comprehensive keyboard run was performed for this finding.
The placeholder's possible fallback presentation does not establish the intended descriptive label.
Coordinator suite passes do not supply a focused accessible-name assertion for these two controls.
No new test or UI change occurred during migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
