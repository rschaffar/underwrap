# RF-0004 — Authenticated activity does not renew inactivity expiry

**Priority:** Medium · **State:** archived · **Resolution:** fixed

**Review aliases:** `B-02` · **Confidence:** High: dependency trace, real HTTP middleware regressions and verified-HTTPS timing with original-cookie replay.

## Why it matters

An active operator could lose authentication at a save-relative deadline despite making successful requests more frequently than the configured inactivity interval.

## Resolution

Fixed in `4b81f8756acae36028de0d70968065749c93c6a6`.
Sliding cookie/store renewal now follows the pinned middleware's activity semantics, including authenticated public requests and non-5xx rejected operations inside the session layer.
Update-only guarded saves prevent late responses from restoring deleted or expired session records.
Idle replay, hardened cookie attributes, revocation and actual backend restart were verified alongside required backend, integration, OpenAPI, SPA and browser checks.

## Caveat

Logout does not cancel admitted work; a late response may emit a stale cookie without restoring authority.
The witnesses cover selected routes, schedules and configured browsers, not universal security assurance.

The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
