# RF-0005 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `L-01`.
Original review owners: `store-lifecycle.md`, `substantive-inputs.md`, `coordinator-notes.md`, `store-lifecycle-hint-check.py`, `verify-L01-shell-tokenization.log`, `report.md`, `input-ledger.md`, `synthesis-verification.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Path admission: `crates/underwrap-backend/src/admin_api/stores/mod.rs:193-221`; construction: `crates/underwrap-backend/src/admin_api/stores/crud.rs:600-605`; presentation: `web/src/pages/stores/dialogs/cleanup-hint.tsx:34-42`.
Existing `crates/underwrap-backend/src/admin_api/stores/crud/tests.rs:211-249` checks the command prefix and an ordinary temporary path, not shell-special operands.

## Recorded observations

The specialist's synthetic shell check shadowed `rm` with an argument-printing function.
The current unquoted form produced two operands for a space-containing path; a quoted control with `--` produced exactly one matching operand.
Recorded filesystem mutations: 0.
Production handler executed: false.

The successful specialist run exited 0 in 0.037 seconds.
An earlier timing-wrapper invocation exited 127 because its timing utility was absent and did not execute the witness.
The coordinator repeated the harmless check with the same 2-versus-1 result and 0 filesystem mutations; its retained timing line was 0.053 seconds.
Neither run deleted data or called an Underwrap handler.

## Reproduction or verification procedure

Normalized reconstruction, not a command executed during migration:

1. Choose a synthetic absolute path containing one internal space, such as `/synthetic data/store_example`; no such directory needs to exist.
2. Construct the current suggested command as unquoted `rm -rf` followed by that path.
   Construct a control using `rm -rf --` followed by a proper shell-quoted representation of the same path, for example Python `shlex.quote`.
3. Evaluate each only inside a fresh shell where the first statement defines `rm` as an inert function printing its received arguments.
   Do not call external rm, sudo or a real filesystem deletion.
   Capture arguments rather than terminal logs.
4. Exclude only the known option arguments from the count.
   Assert current form yields two nonmatching operands and control yields exactly the original path.
   For future regression coverage, extend safe instrumentation to quotes/metacharacters without permitting command side effects.

## Limits and missing evidence

The executable witness establishes tokenization, not an observed deletion incident or a rendered-browser/real-handler test.
Shell-metacharacter consequences are source/language reasoning beyond the exercised whitespace case.
The procedure is fully reconstructible without the historical external helper.
No new source review, shell witness or repository mutation occurred during migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
