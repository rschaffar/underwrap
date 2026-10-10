# RF-0037 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `M-01`.
Original review owners: `coordinator-notes.md`, `dependency-check-results.json`, `source-map-js-advisory.json`, `report.md`, `finding-index.md`, `input-ledger.md`, `synthesis-verification.md`, `synthesis-verification-final.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Locked version/dev flag: `web/package-lock.json:3754-3762`; production sourcemap setting: `web/vite.config.ts:18-20`; static SPA/runtime separation: `flake.nix:91-94`.
Durable public advisory: https://github.com/advisories/GHSA-68fv-2mgg-jv7q.
The recorded response identifies CVE-2026-93749, affected >=1.0.0/<1.2.2 and first patched 1.2.2.

## Recorded observations

These exact portable repo-root commands were executed in the prior review, not during migration:

```sh
nix develop --no-update-lock-file --command npm --prefix web ls --all
nix develop --no-update-lock-file --command npm --prefix web audit --package-lock-only --json
nix develop --no-update-lock-file --command npm --prefix web outdated --json
```

`ls --all` exited 0 in 4.16 seconds.
The live lock-only audit exited 1 in 3.38 seconds with one dev-only upstream High advisory.
`outdated` exited 1 in 5.34 seconds, its normal available-updates indication rather than a product failure.
The coordinator successfully fetched the public GitHub advisory and ran npm explain, establishing the tooling dependency paths described above.
No crafted-source-map execution was performed.

## Reproduction or verification procedure

Normalized future methodology; live advisory responses may change:

1. In a disposable checkout/build environment using the supported toolchain, inspect all source-map-js lock entries and dependency explanations without modifying the lock.
   Compare versions and dev classification with the public advisory.
2. If a current network audit is authorized, rerun the recorded lock-only audit and distinguish advisory exit status from installation failure.
   Do not run an automatic fix as part of verification.
3. For a separately approved patch, choose a compatible patched transitive resolution, update the Nix npm dependency hash and validate installed tree, locked SPA/Nix build and the existing test/browser matrix.
4. Confirm the deployed backend remains static-SPA/Rust runtime rather than assuming advisory severity transfers directly to production.
   Do not run a blocking-input exploit to establish the version match.

## Limits and missing evidence

The audit/advisory is a historical snapshot, not a fresh scan.
No caller-to-vulnerable-parser production path, exploit, measured stall or comprehensive dependency audit was established.
No dependencies, source files or hashes were changed during migration, and no live advisory/service query was made.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
