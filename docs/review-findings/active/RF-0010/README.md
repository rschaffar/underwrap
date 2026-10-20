# RF-0010 — An old form submission can close a reopened draft

**Priority:** Medium · **State:** in-progress

**Review aliases:** `W-05` · **Confidence:** High for the source/control-flow chain; no focused slow-request interaction reproduction.

## Why it matters

Several dialogs remain dismissible during submission.
After dismissal and reopening, an earlier successful request can close the new dialog and discard its draft; the first operation may also commit after apparent cancellation.

## Suggested fix

Give each submission clear dialog-lifetime ownership, preventing dismissal or preventing stale completions from closing a later instance.

## Decision and caveat

Open, Medium-priority form-lifetime defect.
Preserve the existing full-submission guard and error ownership; no remote-cancellation guarantee is selected.
Requests retain their original address.
This is not demonstrated cross-store rebinding, exactly-once behavior, or a reproduced application-wide failure.

RF-0010-only correction is user-authorized; `finding.md` owns scope and evidence.
Verification is in progress, with no integration or remote publication authorized.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
