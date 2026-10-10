# RF-0014 — Packaged-process tests lack failure-path kill and reap ownership

**Priority:** Medium · **State:** open

**Review aliases:** `T-01` · **Confidence:** High from independently inspected child ownership/control flow; no orphan-producing failure was injected.

## Why it matters

A failing packaged-process test can leave a child/listener alive while temporary backing state is removed, contaminating later tests.

## Suggested fix

Install kill-and-reap ownership immediately after spawn and retain temporary state until confirmed child exit, preserving the primary test error.

## Decision and caveat

Open, Medium harness isolation/reliability.
Apply the correction to the unguarded packaged tests, not by weakening assertions.
No orphan was deliberately produced and no production vulnerability was demonstrated.
Other guarded Rust/browser owners and their evidence remain valid within scope.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
