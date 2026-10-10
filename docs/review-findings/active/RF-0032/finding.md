# RF-0032 — Duplicate resource rename is misreported as a network error

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

In `web/src/pages/secrets/dialogs/rename-resource.tsx:44-63`, the dialog builds a set of other known names, trims the candidate, accepts an unchanged name as a no-op, and throws an ordinary Error for a known duplicate before calling `putResourceRename`.

That throw occurs inside the submit function used as the mutation function.
`web/src/hooks/use-form-dialog.ts:81-84` catches the resulting request rejection and calls `describeApiError`.
The non-ApiError fallback at `web/src/lib/error-message.ts:32-33` says: Network error.
Check your connection and try again.

## Consequence and boundary

A correctly rejected local collision is reported as connectivity trouble.
The real cause is neither transmitted to the backend nor shown as an actionable naming error.
This is misleading feedback, not a failure of collision rejection or evidence of changed resource identity/material.

## Correction and alternatives

Move known-name rejection into safe field validation, as Create resource already does at `web/src/pages/secrets/dialogs/create-resource.tsx:35-43`, or use another narrowly classified validation path.
Do not expose arbitrary exception messages to solve one known local case.
Preserve the current-name no-op and backend handling of stale metadata or concurrent collisions; local inventory validation is not CAS.

## Acceptance criteria

- A known duplicate produces a clear collision message and zero rename requests.
- An unchanged name remains a no-op; a valid new name sends the intended request.
- Genuine request failures and backend collision responses retain appropriate safe error reporting, without suppressing invalidation/callback rejections.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
