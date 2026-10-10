# RF-0034 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `W-09`.
Original review owners: `web.md`, `report.md`, `finding-index.md`, `input-ledger.md`, `substantive-inputs.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Reviewed baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
UI: `web/src/pages/stores/dialogs/rotate-key.tsx:146`.
Owners: `docs/backend.md:175-177,282-285` and `docs/operations.md:257-260`.
The rotation admission at `crates/underwrap-backend/src/admin_api/stores/lifecycle.rs:501-522` acquires and revalidates the entry-owned mutex; migration uses that mutex at lines 176-250.

## Recorded observations

The original web review compared the description with maintained backend/operator guidance and deferred broader implementation review to the lifecycle owner.
The final synthesis retained this as Low-priority UI wording debt, not lost locking.
The cited source admission corroborates the shared-mutex distinction; it is not an execution of concurrent lifecycle operations.

## Reproduction or verification procedure

1. Read the complete rotation description, not only the disputed phrase.
2. Compare its serialization statement with the owner paragraphs and the mutex acquisition/revalidation above.
3. After a separately authorized wording correction, check the rendered dialog still conveys non-draining I/O, quiescence, retention and custody limits.

This is a source/contract comparison, not a new rotation test.

## Limits and missing evidence

No concurrent rotation/migration or ordinary-I/O witness was newly executed for this finding.
Neither a suite pass nor corrected prose would establish external custody, erasure, response delivery or power-loss durability.
Existing non-draining behavior is intentional, not an additional defect.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
