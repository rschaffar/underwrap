# RF-0019 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `D-01`.
Original review owners: `daemon-entry.md`, `local-boundary-verification.md`, `local-boundary-commands.md`, `local-boundary-pidfd-lifetime.json`, `input-ledger.md`, `claim-evidence-join.md`, `report.md`, `synthesis-verification-final.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Original daemon/entry review and later independent local-boundary verification agree on the numeric-binding defect.
The latter independently followed the deployed identities, socket admission and complete entry endpoint lifetime.
External Linux v6.5 source distinguishes reference-counted PID-object destruction from IDR removal/reallocation (`kernel/pid.c:105-156,170-279,337-358`) and resolves numeric procfs names using `find_task_by_pid_ns` (`fs/proc/base.c:3435-3467`); these are upstream, not Underwrap repository paths.

## Recorded observations

Two prior external Python witnesses on Linux 7.2.9 forced reuse inside private user/PID/mount namespaces by changing that private allocation cursor.
The pidfd_open witness exited 0 in 0.029 seconds with four true observations.
The SO_PEERPIDFD/socketpair witness exited 0 in 0.028 seconds with five true observations: number reused, replacement readable, original pidfd dead, daemon-style comparisons accepted, retained transport usable.
Both endpoints were deliberately exported before the creator exited.
Neither used an Underwrap binary or the production instance group.

The later verifier ran a different nonprivileged anonymous-socket lifetime check, without namespaces, identity/capability changes or forced reuse.
It exited 0 in 0.039 seconds on Linux 7.2.9.
In both controls, the live fdinfo PID matched the creator and became `Pid: -1` after reap.
Retaining both endpoints preserved transport; retaining only the server while the child owned the sole client produced EOF/EPIPE.
UID/GID were unchanged and `forced_pid_reuse=false`.
This establishes the missing endpoint-retention distinction, not a bypass.

## Reproduction or verification procedure

The original scripts were external, not shipped tests; the following is a normalized methodology, not an exact retained invocation:

1. Inspect the numeric lookup and cross-check sequence, comparing object lifetime with numeric-name allocation in the cited upstream source.
2. For the bounded lifetime control, acquire SO_PEERPIDFD from a live child-created anonymous stream pair, record only PID-match booleans, reap the child and inspect the held fdinfo.
3. Compare deliberate retention of both endpoints with retention of only the server endpoint; observe usable transport versus EOF/EPIPE.
4. Test forced reuse only in a separately authorized disposable namespace, distinguishing that synthetic transport from the actual setgid chain.

## Limits and missing evidence

The fresh control did not force reuse, run Rust peer auth or test socket DAC.
Existing `crates/underwrapd/src/peer_auth/tests.rs:1-500` and `nix/tests/test.py:916-967` lack the successful wrong-object interleaving.
No supported route retaining an admitted client across peer death was established.
The final Low disposition supersedes the original Medium proposal without denying the primitive.
This migration reran none of these checks.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
