# RF-0024 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `X-05`.
Original review owners: `nix-discovery.md`, `documentation-replacement.md`, `report.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The Nix specialist completely read `nix/tests/node.nix` and the module, and inspected `nix/tests/test.py:1800-1881`.
The documentation reviewer independently retained the stale-comment disposition.

## Recorded observations

The comment says distinct executable paths are needed for separate UIDs, while module generation hashes pair identity and the VM assertions require separate identities for same-binary wrappers.
Maintained owners already describe pair identity (`docs/configuration.md:81-101`; `docs/local-protocol.md:56-60`).

This finding was established by source comparison, not a new runtime reproduction.
The coordinator's later existing VM pass does not make the contradictory comment correct.

## Reproduction or verification procedure

1. Read the fixture comment and its thin-shim context.
2. Compare the inputs to target identity generation at `nix/modules/underwrap.nix:619-635`.
3. Inspect the same-binary and cross-instance assertions at `nix/tests/test.py:1800-1881`; retain their narrower executable evidence separately from prose.

## Limits and missing evidence

No production isolation defect follows from the wording.
No runtime behavior change or corrected-fixture validation was performed.
This record does not convert the existing suite into general identity-isolation certification.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
