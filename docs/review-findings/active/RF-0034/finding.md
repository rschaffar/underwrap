# RF-0034 — Rotation dialog understates shared lifecycle serialization

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

Read the confirmation for rotating an unlocked encrypted store.
`web/src/pages/stores/dialogs/rotate-key.tsx:146` says rotation is serialized only against another rotation.

The maintained contract at `docs/backend.md:175-177,282-285` instead states that migration, rotation, update and delete acquire the entry-owned lifecycle mutex; lock/unlock use exact-entry/lifecycle revalidation under their final registry lock.
Rotation shares the mutex but does not drain ordinary I/O.
`docs/operations.md:257-260` preserves that distinction.

## Consequence and boundary

The warning understates existing lifecycle coordination and can give operators the wrong explanation of which operations are serialized.
Its broader quiescence advice remains necessary because ordinary reads/writes are not drained.
This conservative but inaccurate wording is not evidence that the backend lost a lifecycle lock.

## Correction and alternatives

Correct the description to distinguish shared lifecycle serialization from non-draining ordinary I/O.
Preserve warnings about concurrent reads failing or writes being lost, generation retention, disk use, one-time key delivery and independent custody verification.
Do not turn a wording fix into drain-enabled rotation, transactional semantics, automatic cleanup or secure-erasure promises.

## Acceptance criteria

- Compare the revised dialog with both maintained owners and the current lifecycle implementation.
- Ensure it no longer limits serialization to rotation-versus-rotation or claims universal blocking of all store access.
- Retain quiescence, retained-generation and key-custody warnings; verify the relevant rendered description without claiming new concurrency assurance.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
