# RF-0012 — Runtime-base paths are not escaped for tmpfiles syntax

**Priority:** Medium · **State:** blocked

**Review aliases:** `X-01` · **Confidence:** High from actual-module/parser regressions and ordinary/edge-base shipped-chain VM activation.

## Why it matters

Admitted percent, backslash or quote characters can make tmpfiles provision a different runtime path or reject the rule, breaking deployment or invocation.
After [RF-0001](../../archived/RF-0001/README.md), files staging also requires the exact provisioned target runtime directory before fork; descriptor checks do not repair tmpfiles escaping.

## Correction and remaining blocker

Implemented in `70ae50f07f3fa75e2bf5b68234f1cd20ea702258`: encode tmpfiles syntax without changing admitted literal paths, daemon configuration or the privileged chain.
Actual-module/parser cases, both full VM suites, focused module/examples, workspace test/format/Clippy and writeback integration passed.
The broader `nix flake check` failed six unchanged SPA tests, including four timeouts; local `main` selects the identical failing web-test derivation.
No SPA correction is authorized here, and the failure's cause is unestablished.
Keep this record active pending aggregate acceptance rather than implying a fully green gate.
The input is trusted operator configuration, not caller-controlled code.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
