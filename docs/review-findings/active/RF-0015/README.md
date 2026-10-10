# RF-0015 — Parallel fixtures use non-reentrant group lookup storage

**Priority:** Medium · **State:** open

**Review aliases:** `T-02` · **Confidence:** High from source/FFI-lifetime analysis and coordinator verification; no race or sanitizer execution.

## Why it matters

Concurrent fixture lookups can race the shared getgrgid result during Rust pointer/string copying, causing unreliable or unsafe test-process behavior.

## Suggested fix

Use privately owned reentrant NSS output with explicit lifetime/error handling while preserving effective-GID fixture selection.

## Decision and caveat

Open, Medium harness memory-safety/reliability.
No correction or concurrency validation is claimed complete.
This concerns the integration-test helper, not production peer authentication.
No observed race, wrong-group startup or supported-chain authorization bypass was demonstrated.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
