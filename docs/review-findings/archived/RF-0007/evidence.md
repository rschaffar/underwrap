# RF-0007 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `S-03`.
Original review owners: `secret-service.md`, `substantive-inputs.md`, `coordinator-notes.md`, `documentation-replacement.md`, `report.md`, `input-ledger.md`, `claim-evidence-join.md`, `synthesis-verification.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Missing object-server removal: `crates/underwrap-secretsd/src/dbus_service/item.rs:40-54` and `crates/underwrap-secretsd/src/dbus_service/collection.rs:70-81`.
Ignored setter results: `crates/underwrap-secretsd/src/dbus_service/item.rs:95-102,139-144,166-171` and `crates/underwrap-secretsd/src/dbus_service/collection.rs:195-201`.
Missing-item model behavior: `crates/underwrap-secretsd-store/src/store/items.rs:28-45`.

## Recorded observations

The specialist and coordinator independently traced the exported lifetime, false result and successful publication-return path.
Model tests at `crates/underwrap-secretsd-store/src/store/tests.rs:396-451` assert false/None and exact unchanged state on missing objects.
Collection deletion tests at `crates/underwrap-secretsd/src/dbus_service/collection/tests.rs:89-128` inspect the store rather than post-delete registration.
Existing typed item tests cover ACK/reject on extant items, not missing-item setter outcomes.

No fresh post-delete wire reproduction, registry-growth measurement, elapsed time or command exit is recorded for this finding.
The source-derived obsolete-interface accumulation consequence was explicitly preserved by independent synthesis.

## Reproduction or verification procedure

Recorded method: source inspection, not execution.
Future normalized procedure:

1. Prepare a disposable private Secret Service/bus with valid synthetic collection/item IDs, empty/nonsecret material and an identified publication ACK receiver.
   Obtain both object paths and retained interface references.
2. Accept item deletion, then try every item setter using those references.
   Compare response outcome, accepted state and subsequent GetSecret; do not print secret bodies.
3. Repeat after collection deletion for descendant item setters and collection Label.
   Inspect actual exported dispatch as well as model state.
4. Reject a delete and verify state/interface retention; include accepted/rejected setters on an existing item as controls.
5. Repeat bounded create/delete cycles and inspect obsolete object registration directly.
   Reap disposable processes.
   If using a simulated ACK peer, explicitly separate that from real namespace/persistence evidence.

## Limits and missing evidence

The concrete chain is source-confirmed but not freshly wire-executed.
No universal D-Bus/session conformance, cross-resource disclosure, whole-process memory bound or denial-of-service assurance is inferred.
The host ACK is not proof of backend persistence or caller receipt.
No new source review, live-bus experiment or implementation occurred during migration.

## Resolution evidence

Fixed in `80b904ca0167ad9c2c87183045cf1ff60feebd01`, jointly with RF-0006 but with the separate acceptance below.
The final source was checked on x86_64 Linux in the pinned Nix development shell before committing; the archival follow-up changes documentation only.
The complete commands, successful exits and shared review qualifications are recorded in [RF-0006's required checks](../RF-0006/evidence.md#shared-required-checks).

### RF-0007 acceptance

- `crates/underwrap-secretsd/src/dbus_service/wire_tests.rs::accepted_deletions_remove_dispatch_and_retained_handlers_reject_setters` accepts item deletion and collection deletion through an alias, then checks actual registry lookup and exported-path SetSecret, Attributes, item Label and collection Label calls fail.
  Retained interface handlers return UnknownObject for every corresponding setter under the store lock, without publishing extra no-op candidates; accepted state remains unchanged.
  Collection deletion also removes descendants, the invoking alias and the default alias.
- `wire_tests.rs::bounded_create_delete_cycles_leave_no_obsolete_registry_nodes` completes six accepted collection/item create-delete cycles, reusing collection/alias names and replacing random item paths.
  It inspects interface lookup and actual D-Bus Introspect child nodes after deletion, ending with exactly the baseline collection and alias rather than inferring bounded exports from canonical state alone.
- `wire_tests/rollback.rs::rejected_alias_and_delete_changes_preserve_callable_graph_and_state` rejects item and collection deletion, retains complete accepted state and exports, and successfully calls GetSecret and property reads afterward.
- `wire_tests/rollback.rs::extant_object_setters_still_publish_ack_and_reject_on_wire` exercises rejected and accepted SetSecret, Attributes, item Label and collection Label updates on existing objects.
  `transport_failures_preserve_state_and_exports` retains callable item/collection paths and complete accepted state after EOF and wrong-generation deletion responses.
- `wire_tests/concurrency.rs` checks shared-reference property setters under retained interface read guards, registry-read/store-lock ordering, deterministic graph-method exclusion and cancellation of an actual pending accepted reconciliation.
  Registration collision and cancellation child witnesses exit 1 instead of returning ordinary mutation rejection or continuing with incomplete exports.

All seven actual-dispatch witnesses passed both over peer sockets and on a disposable real private bus with synthetic publication responses.
The complete 67-test secretsd suite, 23 store tests, 126 protocol tests, 17 namespace publication controls, RF-0003 tracing/prevalidation controls, Rustfmt, denied-warning workspace Clippy, 114 daemon/backend integration tests and final-source shipped-chain VM check passed.
No integration harness, namespace writeback, raw-storage or identifier-contract changes were made.

### Limits

Synthetic host ACKs do not prove backend durability, namespace receipt or caller receipt.
The unchanged VM scenarios retain shipped-chain evidence, not a new end-to-end alias/deletion persistence or complete D-Bus conformance witness.
Bounded registry reuse is not a whole-process memory bound or denial-of-service assurance.
RF-0012's independently recorded aggregate SPA-check blocker remains separate; no full aggregate pass or waiver is claimed here.
The original discovery evidence remains preserved above.
