# RF-0009 — Text reveal and copy drop a leading UTF-8 BOM

**Priority:** Medium · **State:** archived · **Resolution:** fixed

**Review aliases:** `W-03` · **Confidence:** High: failing-first helper regression, shared-view tests and real-backend Chromium clipboard witnesses.

## Why it matters

The default UTF-8 decoder consumed a leading U+FEFF while reporting success, shortening revealed and copied text without changing stored bytes.
Reusing that text could change a credential or replacement value.

## Correction and disposition

Fixed in `5acc9e9db66d084f02e9098cb4d261ab7e4ec360` by configuring the shared strict decoder to preserve the BOM as value content.
BOM-only and leading-BOM values survive reveal and real clipboard copying for env, files and libsecret; complete stored resources remain unchanged.
Full Vitest, selected coverage, production/backend builds, browser-fixture checks and relevant Playwright flows pass.
Strict malformed-base64/invalid-UTF-8 rejection, empty/missing distinctions, chunk-safe encoding and arbitrary-byte backend storage remain unchanged.

Browser evidence is scoped to the selected desktop Chromium environment, not universal clipboard support or erasure.
The feature is retained without local integration or publication.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
