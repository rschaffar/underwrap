# RF-0025 — Store-parameterized permission helpers seed the default store

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

Call `put_policy(store, ...)` or `upsert_permission(store, ...)` with a non-default store and a resource/pair that has not already been seeded there.
At `crates/integration-tests/src/harness/mod.rs:955-960,1035-1041`, both helpers call `ensure_pair` without forwarding the store argument.
`ensure_pair` at lines 475-477 delegates to `ensure_pair_in_store("test", ...)`, hardcoding the default fixture store.

The actual permission requests do use the supplied store.
`put_policy` first seeds, then lists target-store permissions, removes rows absent from the requested document, and finally upserts the requested rows at lines 961-983.
The similarly named parameters therefore do not describe one coherent store-scoped setup operation.

## Consequence and boundary

Setup can mutate the wrong fixture store while leaving the intended reference absent.
Backend reference checking can correctly reject the later target-store permission request.
A whole-policy replacement can already have deleted target rows before a later upsert fails; this is helper sequencing, not an assertion that the production API promises an atomic whole-policy transaction.

The remap case at `crates/integration-tests/src/tests/client_instances.rs:194-205,227-233` does not expose the problem: it explicitly seeds the target pair and also has the default pair.
Its successful behavior remains useful evidence for that prepared scenario, not for fresh arbitrary-store setup.

## Correction and alternatives

Pass the supplied store to the existing store-aware seeding helper, or explicitly restrict the existing helper API to the default store and require a separate target-store path.
Retain effective resource/pair identity and backend reference validation; do not weaken the backend to compensate for incorrect test setup.
No broader cross-store product behavior change is requested.

## Acceptance criteria

- Start with a non-default store and neither relevant pair present in either store.
- Exercise both permission helpers and check resource/pair/permission state in both stores before and after, using value-free comparisons.
- Confirm intended target seeding and no unrelated default-store mutation, or explicit rejection under a chosen default-only contract.
- Preserve the existing default-store and pre-seeded remap controls; assess partial-failure observations without inventing transactional guarantees.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
