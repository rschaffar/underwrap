# RF-0031 — Pair resource controls lack their intended programmatic labels

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

Open Create wrapper/resource pair and choose an existing resource, or enter a name in new-resource mode.
At `web/src/pages/wrapper-permissions/dialogs/create-pair.tsx:102-177`, the visible Resource `FormLabel` belongs to the separate `mode` field.
That field has no corresponding `FormControl`.
Its individually wrapped radio labels do not label the resource selector or new-name input.

The `resource_id` Select and `new_resource_name` Input each belong to another FormItem, receive their own control ID, and have neither the intended FormLabel nor an explicit `aria-label`/`aria-labelledby`.
The new-name placeholder is an example, not a descriptive association with the Resource label.

## Consequence and boundary

The form looks labelled but its generated label/control relationship does not connect the intended text to these controls.
`web/src/components/ui/form.tsx:94-123` obtains `htmlFor` and control ID from the current FormItem context, so visual proximity does not repair the mismatch.
This can obscure what resource choice or input is requested.
It is not a backend authorization or data-integrity defect.

## Correction and alternatives

Put meaningful labels in each actual control's context or provide an explicit, correct programmatic association.
Preserve the existing/new radio choice, field validation messages and resource-selection behavior.
Do not rely solely on test IDs, placeholders or surrounding visual layout.

## Acceptance criteria

- Assert a meaningful accessible name for the resource selector and new-resource input, including the no-existing-resources case.
- Check mode changes preserve the right association and error linkage.
- Verify label activation/focus and selected keyboard behavior without presenting those checks as comprehensive accessibility certification.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
