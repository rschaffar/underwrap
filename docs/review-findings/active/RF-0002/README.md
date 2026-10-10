# RF-0002 — Dynamic-debug SPA fallback can disclose an outside symlink target

**Priority:** High (conditional) · **State:** open

**Review aliases:** `B-01` · **Confidence:** High: unchanged production handler executed through a router; release controls negative; deployed debug exposure unverified.

## Why it matters

A reachable admin-enabled dynamic-debug backend can return a backend-readable outside symlink target to an unauthenticated requester.

## Suggested fix

Enforce the actual asset boundary in dynamic serving, or require embedding-only artifacts for network serving, while preserving assets and SPA history routes.

## Decision and caveat

Open, High conditional impact.
Normal release embedding is unaffected.
This record neither implements nor authorizes a fix.
Requires a suitable final-component symlink and readable target.
Only synthetic files were used; no sensitive target, real-host exposure or complete debug HTTPS deployment was demonstrated.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
