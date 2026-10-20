# RF-0015 — Parallel fixtures use non-reentrant group lookup storage

**Priority:** Medium · **State:** in-progress

**Review aliases:** `T-02` · **Confidence:** High from source/FFI-lifetime analysis and coordinator verification; no race or sanitizer execution.

## Why it matters

Concurrent fixture lookups can race the shared getgrgid result during Rust pointer/string copying, causing unreliable or unsafe test-process behavior.

## Suggested fix

Use privately owned reentrant NSS output with explicit lifetime/error handling while preserving effective-GID fixture selection.

## Decision and caveat

In progress, Medium harness memory-safety/reliability.
Private-buffer correction and deterministic/concurrent controls are present; required verification and independent archival remain pending.
This concerns the integration-test helper, not production peer authentication.
No observed race, wrong-group startup or supported-chain authorization bypass was demonstrated.

The user authorized this NSS subpart of [RF-0014's bounded shared fixture task](../RF-0014/finding.md#work-ownership).
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
