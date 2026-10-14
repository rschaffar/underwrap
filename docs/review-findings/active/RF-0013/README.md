# RF-0013 — Instance selectors collide with internal generated names

**Priority:** Medium · **State:** open

**Review aliases:** `X-02` · **Confidence:** High on generated collisions and the ns toplevel failure; runtime activation outcome remains unexecuted.

## Why it matters

Valid ns and runtime instance names collide with the namespace wrapper or runtime root.
The former fails evaluation; the latter emits incompatible directory owners.
[RF-0001](../../archived/RF-0001/README.md) places files staging beneath the same provisioned runtime tree but does not separate the colliding generated names.

## Suggested fix

Separate internal generated namespaces or reject/document overlapping selectors and runtime paths before generation.

## Decision and caveat

Open, Medium overall configuration correctness; ns alone is fail-closed and lower impact.
No fix is implemented or authorized.
The coordinator evaluated both cases but did not activate them.
Runtime rule ordering and deployed ownership were not observed; no privilege escalation is established.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
