# RF-0015 — Parallel fixtures use non-reentrant group lookup storage

**Priority:** Medium · **State:** archived · **Resolution:** fixed

**Review aliases:** `T-02` · **Confidence:** High from private-buffer FFI ownership review and deterministic/concurrent controls.

## Impact and correction

Previously, parallel fixture setup copied names from shared `getgrgid` storage.
Effective-GID lookup now uses a private `getgrgid_r` record and buffer through result validation and the owned name copy.
Only ERANGE grows the bounded buffer; missing, invalid and failed lookups reject setup without another identity fallback.

## Disposition and limits

Fixed at `11dd6137279f45078b93fe0dcefe13e8ea8ee05c` with deterministic result/name/error/growth controls, concurrent lookup and real fixture stress, and successful full integration.
Stress supplements the pointer/lifetime argument; no sanitizer execution or production NSS/authentication change is claimed.
This finding's acceptance was assessed independently within [RF-0014's shared task](../RF-0014/finding.md#work-ownership).

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
