# RF-0035 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `C-01`.
Original review owners: `documentation-replacement.md`, `nix-discovery.md`, `input-ledger.md`, `finding-index.md`, `report.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The fresh documentation reviewer inspected the instruction sites, generated entry/helper paths, entry argument handling and actual keygen flags/config-derived path (`crates/underwrap-keygen/src/lib.rs:12-28,262-305`).
The final synthesis accepted C-01 conditionally at Low.

## Recorded observations

The generated entry basename for instance `keygen` and installed helper basename are identical but their directories and programs differ.
The entry parser cannot handle `--instance-config` as a helper flag.
Selected keygen CLI assertions at `crates/underwrap-keygen/src/tests.rs:1-135` support the real helper interface.
These were source/test inspections, not executions.

`nix/tests/wrapper-discovery-eval.nix:34-36,90-103` checks analogous `info` naming and explicit helper separation.
It is useful context, not a keygen/sudo reproduction.
The maintained discovery instructions already use an absolute helper path for precisely this naming class.

## Reproduction or verification procedure

1. Compare the admitted `keygen` selector's generated wrapper with the separately installed system-profile helper.
2. Compare their parsers against the documented `--instance-config` command; these steps reproduce the source reasoning without credentials or execution.
3. In a future disposable deployment, establish sudo's actual executable search order, then verify that the corrected absolute command selects the helper under both orders using a non-installing observation/help path.
4. Keep a noncolliding-selector control and avoid reading or emitting private identity material.

## Limits and missing evidence

No deployed sudo/root command-resolution test was performed, and no universal PATH ordering is claimed.
Documentation syntax/link checks cannot establish runtime selection.
The original unavailable documentation session was replaced by the complete fresh report used here; no evidence from that unavailable session is assumed.
No fix or identity operation was performed by this migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
