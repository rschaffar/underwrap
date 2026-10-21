# RF-0010 — An old form submission can close a reopened draft

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

In `web/src/pages/secrets/dialogs/create-resource.tsx:46-61,98-103`, the hook's success callback is `onClose`, but `AdminDialog` receives no pending `nonDismissible` guard and Cancel remains enabled.
Start a slow submission, dismiss through Cancel/Close or Escape, reopen Create resource and enter a second draft before the original request and its awaited continuation finish.

`web/src/hooks/use-form-dialog.ts:74-100` awaits the mutation, then invalidation, then `opts.onSuccess`.
Its ref guard serializes a submission within that mounted hook instance; it does not identify a newly opened instance as a different owner.
The parent at `web/src/pages/secrets/page.tsx:112,195-196,325-329` conditionally mounts the dialog using a boolean and passes `setCreatingResource(false)` as its close action.
The old continuation can therefore turn off the new instance.

The original review found the same omitted dismissal protection in resource rename; libsecret create-collection, create-item and set-alias; user create/edit/reset-password; store edit/unlock; and wrapper-permission create-pair.
Those are source-identified callers, not separately reproduced interactions.

## Consequence and boundary

A later draft can disappear and a server-side mutation can finish after the interface seemed to cancel it.
Unmounting is not a remote cancellation protocol.
The original request captures its original address, so this is not a claim of cross-store request rebinding.
The concrete failure requires the old request and subsequent awaited work to succeed far enough to invoke its success callback.

Flat entry editing, client enrollment/edit, permission editing and key lifecycle dialogs already have relevant guards.
This finding does not invalidate their bounded behavior or the shared hook's same-instance serialization.

## Correction and alternatives

Use the existing pending lifecycle to block incidental dismissal and explicit footer actions, or give completion callbacks instance ownership so obsolete work cannot dismiss later state.
If dismissal remains supported, communicate that submission may still complete.
Preserve the distinction between request failures and invalidation/callback failures; do not mask those as network failures or promise exactly-once effects.

## Acceptance criteria

- With a deferred request, exercise submit, dismiss, reopen, edit, and old successful completion against Create resource.
- Check Cancel, close control, Escape and incidental navigation according to the chosen lifecycle policy.
- Include deferred invalidation, request rejection, normal success and retry controls.
- Audit the listed callers proportionately and ensure no old completion closes a new draft; do not assume a shared-hook change proves every custom handler safe.

## Work ownership

This file is the single work owner for the user-authorized RF-0010 correction.
Use the existing pending non-dismissal/navigation lock and disable explicit Cancel in the eleven listed form callers.
Gate the hook's success callback on its originating mounted instance, since authoritative inventory changes and libsecret refresh errors can unmount even a guarded dialog.
Continue original-address invalidation and preserve its rejection after unmount.
Preserve ordinary idle dismissal, request-error retry, and observable invalidation/callback rejection; do not change other findings, already guarded callers, or backend behavior.
Unmount and browser closure are not remote cancellation.

- [x] Reconfirm the hook, dialog shell, affected caller/parent paths and existing guarded controls.
- [x] Add deferred-request/deferred-invalidation dialog/page regressions, rejection/retry and normal-success controls, and representative browser evidence.
- [x] Run pinned-environment SPA tests/build and affected browser/fixture checks; record baseline and failing runs without weakening gates.
- [x] Record actual evidence and limits and retain inspected feature commits without integration.
- [ ] Settle the final pinned Nix coverage failure before archival acceptance.

Implementation is committed as `c4aa251eda7dfcfaf254c73e6d0be4b255c74806`.
The final Nix `web-test` timed out unchanged encrypted-store-creation and Unicode-reveal tests at 5000 ms, with 376 passes; local checks and all configured browser checks pass.
An earlier Nix coverage pass does not explain or erase that failure.
The cause remains unestablished, and the separately recorded RF-0012 aggregate blocker is not resolved by this task.
See [resolution evidence](evidence.md#resolution-evidence) for exact checks, negative controls and remaining limits.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
