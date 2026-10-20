# RF-0010 — An old form submission can close a reopened draft

**Priority:** Medium · **State:** blocked

**Review aliases:** `W-05` · **Confidence:** High; source inspection, negative controls, dialog/page regressions and representative Chromium evidence.

## Why it matters

Dismissing a pending form previously allowed its eventual success to close a reopened draft.
Inventory changes and refresh errors can also replace a mounted form independently of dismissal.

## Correction and decision

Implemented in `c4aa251eda7dfcfaf254c73e6d0be4b255c74806`.
Eleven callers use pending dismissal/navigation guards; the shared hook suppresses success callbacks from unmounted instances while preserving original-address invalidation and error ownership.
No submitted backend request is cancelled.

Local Vitest, coverage, build and all configured browser checks pass.
Pinned Nix SPA acceptance remains blocked: the final coverage run timed out unchanged store-creation and reveal tests, despite an earlier pass.
Do not archive until that evidence is settled; do not weaken gates or alter other findings here.

`finding.md` remains the scope owner.
Feature commits and worktree are retained without integration or publication.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
