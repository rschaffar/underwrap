# RF-0037 — Dev-only source-map-js lock selection has a patched advisory

**Priority:** Low · **State:** open

**Review aliases:** `M-01` · **Confidence:** High for locked version/advisory match; no demonstrated production vulnerable-parser path or crafted-input execution.

## Why it matters

Development/build/test tooling resolves vulnerable source-map-js 1.2.1.
Crafted indexed-source-map offsets can synchronously block Node tooling if such input reaches the parser.

## Suggested fix

Refresh the existing transitive selection to patched 1.2.2 or another compatible patched version and update the corresponding Nix npm dependency hash; validate the existing build/test policy.

## Decision and caveat

Open, Low development-tool maintenance.
Upstream High is not demonstrated production High.
No dependency update or major-version policy change is authorized.
The dependency is dev-only, production sourcemaps are disabled and Node is absent from the ordinary backend runtime.
No deployed caller-to-parser path or broker confidentiality failure was shown.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
