# RF-0011 — Failed usage reporting hides a known writeback rejection

**Priority:** Medium · **State:** open

**Review aliases:** `N-02` · **Confidence:** High from independent source/control-flow verification; failure conjunction not executed.

## Why it matters

A denied or failed follow-up UsageSignal loses the specific namespace audit outcome for changes already known to be dropped or rejected.

## Suggested fix

Report the established rejection independently of the later usage-reporting result, retaining safe diagnostics and target status.

## Decision and caveat

Open, Medium observability correctness.
Keep current authorization and terminal-verb policy; no fix is implemented or authorized.
Persistence still fails closed.
A generic diagnostic or separate daemon denial may exist; this is not silent acceptance, a confidentiality flaw or a policy bypass.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
