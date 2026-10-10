# RF-0012 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `X-01`.
Original review owners: `nix-discovery.md`, `nix-discovery-commands.md`, `coordinator-notes.md`, `module-verification-results.json`, `verify-module.nix`, `X01-percent-git-flake.log`, `X01-backslash-git-flake.log`, `X01-quote-git-flake.log`, `report.md`, `claim-evidence-join.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The Nix specialist inspected `nix/modules/underwrap.nix:25-47,165,675-681,756-758` and the lexical-path owner.
The coordinator then evaluated the actual module through a clean Git-backed flake, inspecting `services.underwrap.targetRuntimeDirBase`, failed assertions and `systemd.tmpfiles.rules`.

## Recorded observations

Inputs `/run/underwrap-review/%t`, `/run/underwrap-review/back\x20slash` and `/run/underwrap-review/quo"te` each yielded `failedAssertions=[]` and literal, unescaped base and target-child fields.
Corrected evaluations exited 0 in 8.30, 4.99 and 3.98 seconds respectively.

Separately, systemd-tmpfiles 261.3 was run with `--dry-run --create --inline` against synthetic external paths.
`%t` expanded to `run` within the path, `back\x20slash` decoded to a space-containing component, and an unmatched quote was rejected; the combined command exited 65.
No target directory was created.
These are parser results, not activation observations.

Initial coordinator invocations tried to serialize an unapplied Nix function and exited 1; a corrected but unfiltered path-flake attempt timed out at 180 seconds.
Applying the function using a clean Git-backed reference produced the substantive results above.
These setup failures are not project regressions.

## Reproduction or verification procedure

This normalized recipe is not an exact historical invocation; the original external evaluator is not a repository file.

1. Prepare an external evaluator importing the repository's locked flake/module.
   Use a disposable x86_64 container-style system, a harmless package/target, one wrapper and a non-network HTTPS placeholder; disable hardening only to isolate path generation.
2. Evaluate each literal base and an ordinary control; inspect the configured base, all failed assertions and generated base/child rules.
   Force application of the evaluator function.
3. Using an empty disposable directory and numeric/no-op owner fields, feed equivalent synthetic rules to `systemd-tmpfiles --dry-run --create --inline` and inspect selected names/parser errors.
   Do not remove `--dry-run` on the host.
4. Reserve activation and ownership checks for a separately authorized disposable VM.

## Limits and missing evidence

No new edge configuration was activated.
The parser witness and module evaluation occurred separately, with synthetic configuration rather than live resources.
Existing VM/evaluator passes cover ordinary scenarios and do not repair this admitted parser boundary.
No caller control, privilege escalation or arbitrary root command execution was demonstrated.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
