# RF-0020 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `L-03`.
Original review owners: `store-lifecycle.md`, `substantive-inputs.md`, `coordinator-notes.md`, `documentation-replacement.md`, `report.md`, `input-ledger.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Incorrect text: `web/src/pages/stores/dialogs/cleanup-hint.tsx:29-30`; actual effect: `crates/underwrap-backend/src/admin_api/stores/crud.rs:591-599` and `crates/underwrap-backend/src/store_registry.rs:307-312`.
Owner terminology: `docs/backend.md:24-26,208` and `docs/configuration.md:210-213`.

## Recorded observations

The lifecycle reviewer read the complete consumer and backend deletion flow.
The coordinator independently confirmed the YAML wording versus dynamic JSON registry effect.
The documentation reviewer retained this finding without assigning a duplicate.
No rendered-browser or live deletion experiment was performed for the wording mismatch.

## Reproduction or verification procedure

The recorded method was source inspection, not execution:

1. Read the cleanup-hint sentence and follow the deletion response to registry removal.
2. Compare the registry unlink with the dynamic-registry ownership described in Backend and Configuration; identify that no YAML write occurs.
3. For a future wording regression, render the component with a synthetic cleanup response and assert registry terminology plus retained-data warnings.
   A real destructive cleanup is unnecessary and must not be used as a wording test.

## Limits and missing evidence

No browser interaction, operator mistake or security impact was observed.
This does not claim automatic data deletion or erasure and does not close the adjacent shell-safety finding.
No new source inspection or product execution occurred during migration.

## Resolution evidence

Fixed implementation: `63a68753da4436e33c040dbc01302ac5e3a7f975`.
The shared [RF-0005 verification record](../RF-0005/evidence.md#resolution-evidence) gives the pinned environment, exact commands, results and qualifications for the unchanged source tree committed at that revision.

This finding's separate acceptance is satisfied by:

- Source comparison of `handle_admin_store_delete`, `delete_store_record` and `storage::json_dir_store::delete`: registry unlink and live-registry removal remain the deletion effects; no YAML or backing-tree deletion is added.
  Relative-record handler tests retain canonical and rotation-generation marker files after force-delete.
- `web/src/pages/stores.test.tsx`: the cleanup response renders “The store registry record for scratch was removed”, excludes YAML, and retains manual follow-up, POSIX shell, retained-generation/backup and no-secure-erasure wording.
- `web/e2e/stores.spec.ts`: a real disposable-backend force-delete renders that terminology and warnings, and preserves the returned command through exact browser copying.
  `web/e2e/cleanup-hint.spec.ts` independently pins wording and special-character/newline presentation through an intercepted response.
- The complete 353-test SPA suite, production build, 10 desktop Chromium store/cleanup scenarios, backend suite, Rustfmt, denied-warning workspace Clippy and unchanged OpenAPI checks all exited 0.

The wording now agrees with [Backend](../../../backend.md#deletion) and [Operations](../../../operations.md#decommissioning).
RF-0005 has its own shell-safety acceptance; neither disposition authorizes automatic cleanup.
Browser evidence is selected desktop Chromium behavior, not proof of operator understanding, production recovery, secure erasure or cross-browser support.
