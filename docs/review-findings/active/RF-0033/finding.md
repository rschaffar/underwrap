# RF-0033 — Locked-store delete guidance claims contents were inspected

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

A locked encrypted store can have `actions.can_force_delete=true` even though its contents were not inspected.
`web/src/pages/stores/table.tsx:10-13` turns that action into a tooltip asserting the store contains secrets.
`web/src/pages/stores/dialogs/delete.tsx:23,69-75` makes the same assertion when force is required.

The backend at `crates/underwrap-backend/src/admin_api/stores/crud.rs:539-566` deliberately treats the no-lease locked case conservatively as nonempty for the force decision.
Its specific locked-store error says contents cannot be inspected.
This includes a locked store that happens to contain no resource data.

## Consequence and boundary

An action permission is being mistaken for an inspection result.
The table body already distinguishes known ready/unlocked contents from unknown contents at `web/src/pages/stores/table.tsx:98-100,160-165`; `docs/operations.md:194-201` makes the same distinction.
Operators therefore receive contradictory information about why confirmation is needed.

## Correction and alternatives

Use state-aware wording or wording that accurately covers both known nonempty and unknown contents.
Preserve explicit force confirmation, mapping blockers and the warning that only the registry record is removed while disk data remains for manual follow-up.
Do not infer emptiness from a false contents boolean or unlock a store merely to justify the warning.

## Acceptance criteria

- Render locked stores with unknown contents without asserting inspected secret presence or emptiness.
- Retain distinct known-nonempty and known-empty controls, force confirmation, and mapped-store blocking.
- Keep table, dialog and backend explanations coherent and preserve registry-only deletion semantics.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
