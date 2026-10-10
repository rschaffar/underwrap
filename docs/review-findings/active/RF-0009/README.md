# RF-0009 — Text reveal and copy drop a leading UTF-8 BOM

**Priority:** Medium · **State:** open

**Review aliases:** `W-03` · **Confidence:** High: actual helper reproductions plus inspected reveal/copy callers; clipboard behavior not executed.

## Why it matters

A valid text value beginning with U+FEFF is reported as successfully decoded but displayed and copied without that first character.
Stored bytes remain unchanged; reusing the shortened text can change a credential or replacement value.

## Suggested fix

Preserve the leading character during successful UTF-8 decoding while retaining strict malformed-base64 and invalid-UTF-8 rejection.

## Decision and caveat

Open, Medium-priority exact-text correctness finding; no binary-editor expansion or implemented fix is implied.
Two helper witnesses used different inputs and measurements.
Neither is a real clipboard or browser/backend round-trip test.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
