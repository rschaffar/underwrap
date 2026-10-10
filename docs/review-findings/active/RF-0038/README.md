# RF-0038 — Compound workspace-reservation failure may leave inconsistent recovery state

**Priority:** Low (candidate) · **State:** needs-verification

**Review aliases:** `L-02` · **Confidence:** Medium, conditional source candidate only: no deterministic create-success/metadata-failure/cleanup-failure witness.

## Why it matters

A newly reserved empty migration directory may remain after compounded failures while the live store resumes normally; restart can then mark the same source RecoveryRequired.

## Suggested fix

First verify the compound path.
If confirmed, distinguish failed reservation with no created object from an owned directory whose cleanup cannot be confirmed, without removing collision occupants.

## Decision and caveat

Needs-verification, Low candidate; the sole unresolved candidate, not an accepted incident.
No fix or recovery behavior change is authorized.
No compound-failure execution, secret-bearing residue, data loss or confidentiality failure was demonstrated.
The candidate concerns empty pre-stage residue, not retained rotation generations or general durability.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
