# RF-0030 — Migration retry policy conflicts with authoritative-reconciliation guidance

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

`web/src/pages/stores/dialogs/migrate.tsx:73-80,97-135` derives eligibility from the current cached store row and starts invalidation in the background after a migration result.
The request's `finally` clears `mutationPending` independently of reconciliation.
`web/src/lib/query.ts:16-23` deliberately launches refresh without extending the mutation lifecycle and catches refresh rejection.
`web/src/pages/stores/page.tsx:23-25,48-53` supplies the row from query data, not a separately confirmed post-attempt snapshot.

If submission returns an ambiguous error and refresh stalls or fails, that row can still describe the original available, migratable source.
The retained-key retry, or encrypted-to-plain retry, can become eligible before authoritative reconciliation finishes.

## Consequence and boundary

The UI's eligibility differs from `docs/operations.md:219-222`: retain the same key, refresh authoritative state, and retry only if the stable ID still names the original eligible source.
A committed target, missing, busy, locked or recovery-required row must instead be reconciled.

Contrary evidence is central: `web/src/pages/stores.test.tsx:923-983` explicitly expects retained-key retry and discard controls enabled during a never-resolving refresh.
Lines 1033-1138 also begin with retry enabled while refresh is unresolved, then disable it when missing/busy/locked data is supplied and re-enable it for the original eligible row.
This is intentional tested behavior, not simply an omitted guard or missing test.

Backend guards at `crates/underwrap-backend/src/admin_api/stores/lifecycle.rs:176-250` still reject the same target backend, revalidate exact entry/mutex identity, and enforce lifecycle admission.
Key retention remains available.
No unsafe backend migration, discarded key, or committed corruption follows from these UI observations.

## Correction and alternatives

One option is to distinguish unresolved/failed reconciliation from a confirmed eligible source and keep mutation retry unavailable until a suitable refresh resolves.
Another is to retain stale-state retry deliberately and reconcile the operator instructions and UI explanation with that policy.
Neither option is pre-approved.
Retain key visibility, explicit acknowledged discard and backend guards; do not couple safe key custody to a refresh that may never finish.

## Acceptance criteria

- Record the chosen retry contract in maintained guidance and align implementation and tests.
- Cover both stalled and failed refresh, not only a successfully updated list.
- Preserve positive eligibility and negative committed-target/missing/busy/locked/recovery-required controls in both migration directions.
- Retain the same ambiguous key and acknowledged discard without asserting response delivery, custody, or erasure.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
