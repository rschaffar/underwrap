# RF-0004 — Authenticated activity does not renew inactivity expiry

**Priority:** Medium · **State:** open

**Review aliases:** `B-02` · **Confidence:** High: application/dependency trace and exact-release-backend HTTPS timing with explicit original-cookie replay.

## Why it matters

An active operator can lose authentication at a save-relative deadline despite making successful requests more frequently than the configured inactivity interval.

## Suggested fix

Make session refresh match the selected inactivity policy while retaining expiry and revocation, or explicitly choose and document absolute expiry.

## Decision and caveat

Open, Medium workflow correctness.
No fix or alternative policy is selected or authorized.
Not an authentication bypass or rollback of committed operations.
Genuine-idle expiry and logout/password/restart controls were not freshly exercised by this timing witness.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
