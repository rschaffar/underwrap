# RF-0013 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `X-02`.
Original review owners: `nix-discovery.md`, `nix-discovery-commands.md`, `coordinator-notes.md`, `module-verification-results.json`, `verify-module.nix`, `X02-runtime-git-flake.log`, `X02-ns-git-flake.log`, `report.md`, `claim-evidence-join.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The original Nix review identified direct string/key identities in `nix/modules/underwrap.nix:84-87,125-128,199-217,675-681,692-695,897-903`.
The coordinator evaluated the actual module using a clean Git-backed flake with a synthetic package and wrapper.

## Recorded observations

The `runtime` rules evaluation exited 0 in 3.78 seconds, returned no failed assertions, and emitted both `d /run/underwrap-runtime 0755 underwrap-runtime underwrap-runtime -` and `d /run/underwrap-runtime 0755 root root -`.
The state-directory and target-child rules did not remove that conflict.

The `ns` forced-toplevel evaluation exited 1 in 3.22 seconds with the specific assertion: `The security.wrappers.underwrap-ns wrapper is not valid: setuid/setgid and capabilities are mutually exclusive.` This is an expected finding witness, not a generic build failure.

A separate synthetic tmpfiles dry-run placed two owners on one external directory: numeric owner/group 2001 first, root second.
It reported that the second rule was ignored.
That demonstrates parser conflict handling for the supplied order, not the exact final NixOS activation order.
No directory was created.

Earlier unapplied-function invocations exited 1 and the shared unfiltered path-flake setup timed out; corrected Git-backed evaluations above are the substantive evidence.
The specialist's evaluation-pending wording is superseded, while activation remains missing.

## Reproduction or verification procedure

1. In an external evaluator using the locked module, configure a harmless wrapper and select `ns`; force `config.system.build.toplevel.drvPath`, recording the specific assertion.
2. Select `runtime` with the default base; inspect failed assertions and all tmpfiles rules for duplicate paths/owners.
3. Compare a noncolliding instance and custom-base control.
4. If testing activation, use only an authorized disposable VM and check actual directory owner/mode plus daemon socket readiness.
   Do not infer them from serialized rules.

This is a normalized method; no external evaluator is assumed to exist in the repository.

## Limits and missing evidence

Neither edge configuration was activated.
No deployed directory takeover, account fusion or authorization bypass was observed.
The fresh ordinary VM pass and selector-grammar/hash-collision evaluators remain useful, but do not cover these internal generated-name collisions.

## Post-RF-0001 assessment

Assessed revision: `7d4b1d30794171956031cef45b805b0c46500619`.
Source/diff inspection of the RF-0001 range from `a00cb9b40042064bfff9ca64fca95dab70d3f57a` confirms that `nix/modules/underwrap.nix` and its colliding generated names/rules are unchanged.
`directory_credential.rs::prepare_directory_mounts` now selects a checked target-child runtime object for files staging, but does not validate or resolve the two competing runtime-base ownership contracts.

[RF-0001 correction verification](../../archived/RF-0001/evidence.md#resolution-evidence) exercised noncolliding instance/runtime configurations only.
A root-protected legacy staging basename is not an `ns`/`runtime` selector-collision control.
This follow-up performed no collision activation or new evaluator execution; the original specific evaluation failure, duplicate-rule evidence and deployed-ordering gap remain.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
