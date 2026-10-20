# RF-0014 — Packaged-process tests lack failure-path kill and reap ownership

**Priority:** Medium · **State:** archived · **Resolution:** fixed

**Review aliases:** `T-01` · **Confidence:** High from source ownership review and isolated packaged-process witnesses.

## Impact and correction

Previously, failing packaged tests could lose child ownership before temporary backing state was released.
`PackagedChild` now owns each acquisition and shared state immediately, keeps borrowed output waits cancellation-safe, and confirms fallback reap or retains state with metadata-only diagnostics.
Existing bootstrap owners use the same guarded path.

## Disposition and limits

Fixed at `11dd6137279f45078b93fe0dcefe13e8ea8ee05c` with 17 externally supervised acquisition/readiness/use failures, identity-bound exit/reap and listener/state checks, setup/output/cancellation controls, simulated cleanup-error precedence and successful full integration.
This is direct-child fixture isolation, not production shutdown or cleanup after process abort.
The transient holder behind one earlier parallel flock-control failure was not identified; the exact controls now run in isolated processes.

[Technical finding and shared work owner](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
