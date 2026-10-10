# RF-0023 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `X-04`.
Original review owners: `nix-discovery.md`, `nix-discovery-commands.md`, `test-harness.md`, `documentation-replacement.md`, `report.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The Nix specialist completely read `flake.nix` and `crates/integration-tests/run`, then inspected existing info/backend derivations and a workspace control.
The documentation reviewer retained the finding alongside otherwise consistent build prerequisites.

## Recorded observations

The per-crate derivations emit `cargoWithProfile build/test --package ...` without `--locked`; the inspected workspace derivation includes `--locked` for build/test.
The filtered source referenced by the inspected artifacts matched 280 selected current repository files, with none missing/different.
That supports attribution of the source/command observation, not a fresh build or complete dependency audit.

The supported runner's source ends in `cargo test -p integration-tests` with forwarded arguments.
Neither the specialist nor coordinator executed a deliberate manifest/lock discrepancy for this finding.
The later normal validation pass does not close that counterfactual gap.

## Reproduction or verification procedure

1. Inspect `cargoExtraArgs` in `flake.nix:137-141` and the final command in `crates/integration-tests/run:69-70`.
2. Inspect evaluated derivation command fields for per-crate build/test and compare with the workspace `--locked` control.
   This is source/command inspection, not executing Cargo.
3. For behavioral verification only after authorization, use an external disposable source copy with the required pinned toolchain/vendor/artifact preparation, test one otherwise-resolvable lock mismatch and an unchanged control.
   Record rejection cause and lock preservation, not merely a generic nonzero result.

## Limits and missing evidence

No stale-lock experiment, dependency re-resolution or repository lock mutation was executed.
Existing artifact inspection is not fresh build evidence.
The issue does not discredit fixed Nix inputs or imply a production vulnerability.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
