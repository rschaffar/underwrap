# RF-0007 — Deleted D-Bus objects acknowledge setters without changing state

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

Load/create an item, successfully delete it or its collection, then call SetSecret, Attributes or Label through its exported path or a cached interface.
Item deletion updates the candidate store but does not remove the exported interface (`crates/underwrap-secretsd/src/dbus_service/item.rs:40-54`).
Collection deletion likewise leaves collection and descendant item interfaces (`crates/underwrap-secretsd/src/dbus_service/collection.rs:70-81`).

The model correctly returns false without mutation when an item/collection is missing (`crates/underwrap-secretsd-store/src/store/items.rs:28-45`).
Setters ignore that result, publish the unchanged candidate and return success after ACK (`crates/underwrap-secretsd/src/dbus_service/item.rs:95-102,139-144,166-171`).
The collection label setter has the analogous behavior (`crates/underwrap-secretsd/src/dbus_service/collection.rs:195-201`).
A later GetSecret fails because the supposed update was never stored.

## Consequence and boundary

An honest client holding an interface across deletion can believe a credential/property update succeeded when no backing state was changed.
No race is necessary.
Repeated accepted create/delete cycles also accumulate obsolete item interfaces even when canonical live state remains small; this is retained as a resource-lifetime consequence, not promoted to a demonstrated DoS exploit or universal availability promise.

The relevant boundary is target call to stale object to missing model lookup and ACK.
`docs/security.md:17` and `docs/local-protocol.md:230-231,262-264` concern truthful accepted-state mutation.
Publication still sends and commits precisely its candidate; the defect is preparing a successful no-op for an absent object.
This is not the documented backend last-writer-wins behavior.

It differs from [RF-0006](../RF-0006/README.md): stale aliases can operate on the wrong existing collection, whereas this finding acknowledges mutation of absent state.
Neither establishes another resource's exposure or defeats the trusted-target model.

## Correction and alternatives

Keep object-server lifetimes consistent with accepted deletions, including collection descendants, and explicitly reject missing backing objects under the mutation lock.
Do not unregister before a deletion has been accepted or let rejected publication remove a valid object.
Keep ACK-before-live-state mutation and rollback guarantees.
A defensive missing-object check remains necessary even if ordinary registration cleanup is corrected, because callers can retain interface references.
This is not a ban on valid idempotent operations on existing objects.

## Acceptance criteria

- After accepted item or collection deletion, calls through exported paths and retained interfaces must not acknowledge discarded setters.
- Cover SetSecret, Attributes, item Label and collection Label; check exact accepted state remains unchanged on absence.
- Rejected deletion must retain both backing state and callable interface.
  Existing-item accepted/rejected setter behavior must remain correct.
- Repeated accepted create/delete must not retain obsolete exported interfaces; inspect registry/dispatch rather than infer from small canonical state.
- Preserve publication rejection/transport-failure rollback.
  No fixed-build or new wire regression has run.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
