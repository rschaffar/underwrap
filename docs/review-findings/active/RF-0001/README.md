# RF-0001 — Replaceable staging root can substitute collected files

**Priority:** High · **State:** open

**Review aliases:** `N-01` · **Confidence:** High source confidence, supported by a prior synthetic mount primitive; no packaged distinct-UID exploit.

## Why it matters

A caller who preclaims the staging directory can replace its pathname so a legitimate writable invocation collects caller-supplied files.
Earlier replacement can also move materialization outside the private tmpfs.

## Suggested fix

Keep stable authority to the intended private filesystem objects throughout materialization, binding and collection.

## Decision and caveat

Open, High for files integrity and containment.
Preserve writable projections and backend policy; no correction has been implemented or authorized.
Requires a caller-owned preclaimed root and an accepted invocation; persistence also requires writable policy.
No packaged cross-UID exploit, direct secret recovery, arbitrary privileged write or bus interception was demonstrated.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
