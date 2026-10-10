# RF-0022 — Selected proc probes confuse access denial with other errors

**Priority:** Low · **State:** open

**Review aliases:** `X-03` · **Confidence:** High from inspected assertions and synthetic predicate controls; no new kernel-boundary attack execution.

## Why it matters

Selected ATTACH and ancestor-memory witnesses can pass on invalid-address, vanished-process or generic read errors rather than permission denial.

## Suggested fix

Require the intended permission errno and identity-bound target liveness, separating open denial from read failure.

## Decision and caveat

Open, Low test-evidence quality.
Preserve separate UID, READ, peer and ancestor controls; no fix is implemented or authorized.
This does not demonstrate a production leak or invalidate the entire VM suite.
The recorded reproduction exercised assertion predicates, not kernel access.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
