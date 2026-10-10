# RF-0022 — Selected proc probes confuse access denial with other errors

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

`nix/tests/peer-probe.py:47-68` labels every negative process_vm_readv return as `VM_READV_DENIED(errno=...)`; its ptrace branch similarly labels any negative return as denial.
The VM consumers at `nix/tests/test.py:1114-1130,1456-1473` check token presence rather than the expected permission errno or identity-bound liveness across the probe.

An invalid speculative address can return EFAULT even after permission checks allow access; a vanished target can produce ESRCH.
Both are therefore counted as success by these particular assertions.
The related ancestor witness at `nix/tests/ancestor-attack.py:75-83` and `nix/tests/test.py:2049-2056` accepts generic `MEM_ERROR`: a read from address zero can fail after opening `/proc/<pid>/mem` successfully.
Failure to recover bytes is not equivalent to denial of opening or accessing the process.

## Consequence and boundary

The defect weakens evidence for caller-to-target process-memory confidentiality under `docs/security.md:44-70`.
The conceptual attacking process is a caller-owned peer/ancestor, but no actual production access was established by this review.
Separate dedicated-UID checks and other READ-denial witnesses remain useful.
Correct syscall denials and source hardening must not be discarded merely because these consumers are too permissive.

The synthetic check deliberately used EFAULT and ESRCH labels for both token types to test the predicates.
It does not claim every supplied errno is a naturally reachable result for every specific syscall.
Its decisive observation is that the assertions cannot distinguish authorization errors from the supplied alternatives.

## Correction and alternatives

Require the documented permission-denial errno for the intended live target, bind liveness to identity rather than an unqualified reusable PID, and distinguish memory-open failure from subsequent read failure.
Keep peer versus ancestor and READ versus ATTACH as separate evidence.
Changing production policy is not implied.

## Acceptance criteria

- Dead-PID and invalid-address controls must not count as successful boundary denials.
- A permission-denied result for a still-live intended target must remain accepted; an allowed-memory control must be distinguishable without logging resource data.
- An ancestor test must reject the case where memory opens successfully but the chosen read fails.
- Run the focused VM witnesses after any authorized correction; retain unrelated suite evidence rather than claiming blanket invalidation.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
