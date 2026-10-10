# RF-0018 — Permission-cleanup diagnostics mislabel unavailable store states

**Priority:** Low · **State:** open

**Review aliases:** `A-02` · **Confidence:** High: independently confirmed source branches and owner-contract comparison; no fresh Busy/RecoveryRequired execution.

## Why it matters

After committed client remap/deletion, failed best-effort cleanup can tell an operator to unlock a store that is actually Busy or RecoveryRequired.
A comment also incorrectly promises self-healing.

## Suggested fix

Report the actual lifecycle reason and appropriate manual follow-up while retaining best-effort cleanup and committed authority changes.

## Decision and caveat

Open, Low diagnostic correctness.
No automatic repair, transaction or migration-concurrency change is selected or authorized.
Stale rows are an intentional possible outcome of postcommit cleanup.
The finding does not restore old-store access, grant permissions or undermine lifecycle denial.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
