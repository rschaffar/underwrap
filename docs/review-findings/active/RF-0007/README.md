# RF-0007 — Deleted D-Bus objects acknowledge setters without changing state

**Priority:** Medium · **State:** in-progress

**Review aliases:** `S-03` · **Confidence:** High: source review, post-delete wire dispatch and retained-handler regressions.

## Why it matters

A cached item/collection interface remains callable after accepted deletion and can report a successful setter while discarding the change.
Repeated create/delete also retains obsolete interfaces.

## Suggested fix

Align exported lifetimes with accepted deletion and reject setters on absent backing objects under the mutation lock, preserving ACK ordering and rollback.

## Decision and caveat

The user-authorized joint correction is owned by [RF-0006](../RF-0006/finding.md#work-ownership).
Accepted deletion removes exports and retained setters reject absent state under the store lock; required validation is in progress.
The host still commits the exact candidate it receives.
This is not a publication-validation bypass, cross-resource leak or demonstrated denial-of-service attack; obsolete-interface accumulation is a bounded lifetime observation.

Wire regressions cover rejected deletion and bounded registry reuse with simulated host ACKs, not backend durability.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
