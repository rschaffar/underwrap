# RF-0026 — Readiness loops can wait about 26 minutes rather than 60 seconds

**Priority:** Low · **State:** archived · **Resolution:** fixed

**Review aliases:** `T-05` · **Confidence:** High from deadline source review and measured stalled/prompt HTTPS controls.

## Impact and correction

Previously, 3,000 request timeouts could extend readiness far beyond its described 60-second window while retaining the startup reservation.
Both admin and enrolled-runtime probes now use an absolute elapsed deadline, constrain requests and sleeps to remaining time, and observe backend completion.

## Disposition and limits

Fixed at `11dd6137279f45078b93fe0dcefe13e8ea8ee05c` with externally bounded stalled-TLS and real prompt-success controls, failed-backend observation, reservation ownership/release checks and normal awaited shutdown.
The measured 240-ms probes took approximately 243 ms each.
Readiness does not bound blocking setup, joins, whole-fixture lifetime or production availability.
This finding's acceptance was assessed independently within [RF-0014's shared task](../RF-0014/finding.md#work-ownership).

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
