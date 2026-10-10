# RF-0022 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `X-03`.
Original review owners: `nix-discovery.md`, `nix-discovery-commands.md`, `coordinator-notes.md`, `claim-evidence-join.md`, `report.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The Nix specialist read the complete peer/ancestor helpers and the relevant VM ranges.
The peer probe itself comments that EPERM, not EFAULT, is the expected process_vm_readv denial; its output/consumer contract does not enforce that distinction.

## Recorded observations

An external Python AST check parsed `nix/tests/test.py`, selected `local_target_uid_isolation`, extracted the four assert expressions mentioning `probe`, and evaluated them against synthetic dictionaries.
Both `errno.EFAULT` (14) and `errno.ESRCH` (3) satisfied all four assertions.
The command exited 0; duration was not measured.
No VM function, attack program or kernel read was executed by that check.

The coordinator later built and passed the existing VM derivation in 174.22 seconds.
That ordinary suite pass is retained, but does not repair assertions which also accept the synthetic non-denial outcomes.
The final claim/evidence join explicitly preserves dedicated-UID/other READ evidence separately.

## Reproduction or verification procedure

To reconstruct the exact logical check without executing an attack:

1. Parse `nix/tests/test.py` with Python `ast.parse`; select the top-level function `local_target_uid_isolation`.
2. Select its `ast.Assert` nodes whose unparsed test contains `probe`; confirm there are four at the reviewed baseline.
3. For EFAULT and ESRCH, supply a dictionary with keys `PTRACE_DENIED` and `VM_READV_DENIED`, each containing its corresponding `...DENIED(errno=<number>)` string.
   Evaluate only the extracted assert expressions.
4. Record whether each expression accepts; never invoke the VM function or syscall helpers in this predicate check.

Future kernel/VM validation needs the positive and negative controls in the acceptance criteria.

## Limits and missing evidence

The ancestor weakness is source-inspected; no new ancestor wire/kernel witness was executed.
No leak, failure of dedicated UIDs, or universal weakness of all proc probes is established.
This migration executed no predicate or VM check.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
