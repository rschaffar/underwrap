# RF-0007 — Deleted D-Bus objects acknowledge setters without changing state

**Priority:** Medium · **State:** open

**Review aliases:** `S-03` · **Confidence:** High: independently confirmed exported-object lifetime and ignored missing-object results; no fresh post-delete wire execution.

## Why it matters

A cached item/collection interface remains callable after accepted deletion and can report a successful setter while discarding the change.
Repeated create/delete also retains obsolete interfaces.

## Suggested fix

Align exported lifetimes with accepted deletion and reject setters on absent backing objects under the mutation lock, preserving ACK ordering and rollback.

## Decision and caveat

Open, Medium state/outcome correctness.
No implementation or broad protocol redesign is authorized.
The host still commits the exact candidate it receives.
This is not a publication-validation bypass, cross-resource leak or demonstrated denial-of-service attack; obsolete-interface accumulation is a bounded lifetime observation.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
