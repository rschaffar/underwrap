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

## Post-RF-0001 assessment

Assessed revision: `7d4b1d30794171956031cef45b805b0c46500619`.
Source/diff inspection of the RF-0001 range from `a00cb9b40042064bfff9ca64fca95dab70d3f57a` confirms that `nix/modules/underwrap.nix` is unchanged.
`directory_credential.rs::prepare_directory_mounts` now requires the exact target runtime directory and checked visible ownership/mode before attachment; `main.rs` calls it before fork for files projection.
This extends the direct provisioning dependency to files-only invocations, without changing the demonstrated tmpfiles parser mismatch.

The ordinary runtime-base shipped-chain controls passed during [RF-0001 correction verification](../../archived/RF-0001/evidence.md#resolution-evidence).
Those recorded executions did not activate percent/backslash/quote bases and are not acceptance evidence for this finding.
This follow-up ran no edge-base activation or new invocation test; the original discovery results and activation gap remain.

## Resolution evidence

Correction implemented at `70ae50f07f3fa75e2bf5b68234f1cd20ea702258`, based on local `main` at `c7c3c3d771fedffb1755942e55a219e6c1afb827`.
The tested runtime/module/harness inputs match that correction commit; subsequent finding-record changes do not change them.
The record remains active and blocked on the aggregate failure below, not on a demonstrated remaining tmpfiles mismatch.
No integration, remote publication or cleanup was performed.

### Correction and regression evidence

`nix/modules/underwrap.nix::tmpfilesLiteralPath` C-escapes backslashes and both quotes, then doubles percent signs before emitting each complete base/child path field.
The daemon retains its original literal base; path admission, `/run/user` exclusion, identities and privileged Rust chain are unchanged.
`nix/tests/runtime-tmpfiles-cases.nix`, `runtime-tmpfiles-eval.nix` and `runtime-tmpfiles-parser.py` retain ten actual-module cases: default, ordinary custom, root, percent specifiers/trailing percent, backslash escapes/trailing backslash, each quote, Unicode, punctuation and the combined path.
Each forces successful assertions/toplevel evaluation, checks literal daemon JSON, and compares exact parser-selected base/child names using pinned systemd 261.2 with `--dry-run --create --root=… --inline` and synthetic account metadata.
No host activation occurred.

Before correction, the actual-module regression exited 1 because the backslash rules failed tmpfiles parsing.
Separately running every baseline-generated case with the retained parser harness reproduced failures for percent, backslash, each quote and combined syntax, while default/custom/root/Unicode/punctuation controls passed.
After correction all ten passed.
An initial post-correction test expectation omitted the sandbox-root trailing slash for the `/` case; fixing that test expectation, not production admission, resolved the harness failure.

The ordinary `vm-test` retains `/run/underwrap-test-runtime`.
`runtime-tmpfiles-vm-test` runs the same complete shipped-chain script with the combined percent/backslash/both-quotes literal base.
Both passed activated base/child existence and exact root `0755`/target `0700` ownership/modes; the edge VM additionally compared the complete ancestor/child tree to exclude differently interpreted paths.
The target runtime-path witness compares exact `XDG_RUNTIME_DIR` against the root-owned target table.
Both suites retained RF-0003's real libsecret startup/lookup regression and all four RF-0001 files-only staging controls: writable changed, writable unchanged, read-only changed and protected-root.
Those controls observed descriptor-selected tmpfs/noswap staging, distinct UID, bare attachment ownership/mode and exact backend/terminal outcomes at the configured path.

### Executed checks

Commands ran in the isolated x86_64-linux worktree using pinned flake inputs; elapsed durations were not separately measured unless stated.
All Nix builds used `--no-link --print-out-paths`.

| Command or check selection | Result and scope |
| --- | --- |
| `nix build .#checks.x86_64-linux.runtime-tmpfiles-eval` | Exit 0; all ten module/daemon/parser round trips. |
| `nix build .#checks.x86_64-linux.frontend-security-assertions-eval .#checks.x86_64-linux.frontend-backend-url-eval .#checks.x86_64-linux.frontend-swap-posture-eval .#checks.x86_64-linux.wrapper-discovery-eval .#checks.x86_64-linux.examples-nixos-eval` | Exit 0; focused production predicates, both shipped system builds and discovery. Exact `/run/user` and descendant false assertions and rejected toplevels remain intact. |
| `nix build .#checks.x86_64-linux.workspace-test .#checks.x86_64-linux.workspace-fmt .#checks.x86_64-linux.workspace-clippy` | Exit 0; workspace tests with packaged prerequisites, Rustfmt and denied-warning Clippy. |
| `nix build .#checks.x86_64-linux.vm-test .#checks.x86_64-linux.runtime-tmpfiles-vm-test` | Exit 0; both complete scripts reported `ALL TESTS PASSED`, including activation and all four staging provenance controls. |
| `CARGO_TARGET_DIR="$PWD/target" ./crates/integration-tests/run tests::wrapper_writeback` | Exit 0; 12 passed, 102 filtered, test execution 4.35 seconds. |
| `nix flake check --all-systems --no-build` | Exit 0; both supported architectures evaluated, not AArch64 execution. |
| `python3 -m py_compile nix/tests/runtime-tmpfiles-parser.py nix/tests/test.py`, relative documentation-link path validation and `git diff --check` | Exit 0; syntax, local path existence and whitespace checks, not browser link verification. |
| `nix flake check` | Exit 1 in unchanged `web-test`; six failures and 347 passes, described below. No aggregate pass is claimed. |

A separate read-only patch review found no concrete RF-0012 defect; it is not an independent security audit.

### Aggregate blocker and limits

The full aggregate's web-test derivation is `/nix/store/lp8bbviipqf52q4rfn2qjiq2vv189w02-underwrap-web-test-0.1.0.drv`.
Read-only evaluation of current local `main` selected that identical derivation, and `git diff --quiet c7c3c3d771fedffb1755942e55a219e6c1afb827 -- web crates Cargo.lock Cargo.toml` confirmed no Rust/SPA input changes in this correction.
The failed tests were two operator-workflow cases, three store cases and one permission-column case; four timed out at 5000 ms, one observed an unchanged store-selection URL, and one further store reconciliation case failed.
The SPA test run reported 73.12 seconds.
These observations do not establish why the tests failed or justify silently retrying, weakening tests or claiming the aggregate passed.
No SPA implementation or test changes are included; aggregate acceptance remains for follow-up outside this bounded correction.

The dry-run parser uses synthetic account metadata; real provisioning and descriptor-staging evidence comes separately from the two disposable VMs.
The edge VM activates the combined fixture, while individual edge fixtures are parser/evaluation evidence.
One pinned x86_64-linux VM kernel does not establish all-kernel/systemd-version or AArch64 runtime support, and the lexical contract remains distinct from symlink/realpath equivalence.
RF-0013 and other parallel findings are unchanged.
