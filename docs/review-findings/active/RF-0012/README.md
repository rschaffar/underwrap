# RF-0012 — Runtime-base paths are not escaped for tmpfiles syntax

**Priority:** Medium · **State:** open

**Review aliases:** `X-01` · **Confidence:** High from real-module admission/rule evaluation and synthetic tmpfiles parser witnesses; edge configurations not activated.

## Why it matters

Admitted percent, backslash or quote characters can make tmpfiles provision a different runtime path or reject the rule, breaking deployment or invocation.

## Suggested fix

Round-trip every admitted literal through tmpfiles escaping, or explicitly reject unsupported syntax with a documented assertion.

## Decision and caveat

Open, Medium configuration-generation correctness.
Preserve configurable runtime bases and existing path restrictions; no fix is implemented or authorized.
The input is trusted operator configuration, not caller-controlled code.
Module evaluation and parser dry-runs are not NixOS activation or evidence of root execution.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
