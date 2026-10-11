# RF-0001 — Replaceable staging root can substitute collected files

**Priority:** High · **State:** in-progress

**Review aliases:** `N-01` · **Confidence:** High source confidence, supported by a prior synthetic mount primitive; no packaged distinct-UID exploit.

## Why it matters

A caller who preclaims the staging directory can replace its pathname so a legitimate writable invocation collects caller-supplied files.
Earlier replacement can also move materialization outside the private tmpfs.

## Suggested fix

Create one `noswap` tmpfs through a protected runtime attachment and retain directory handles throughout materialization, binding and collection.

## Decision and caveat

High for files integrity and containment; correction in progress.
Preserve writable projections and backend policy; acceptance verification remains pending.
Requires a caller-owned preclaimed root and an accepted invocation; persistence also requires writable policy.
No packaged cross-UID exploit, direct secret recovery, arbitrary privileged write or bus interception was demonstrated.

The correction is authorized with scope owned by `finding.md`; implementation and acceptance verification are in progress.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
