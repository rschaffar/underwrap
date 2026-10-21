# RF-0006 — Accepted alias changes leave exported D-Bus routing stale

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

Start a private Secret Service with alias A pointing to collection C1 and another existing collection C2.
Startup registers `/org/freedesktop/secrets/aliases/A` as a CollectionInterface holding a copied C1 ID (`crates/underwrap-secretsd/src/main.rs:220-230`).
Successfully call SetAlias(A, C2), including a matching host publication ACK.

SetAlias changes the accepted candidate's alias map but never updates/removes/registers alias interfaces (`crates/underwrap-secretsd/src/dbus_service/service.rs:198-232`).
ReadAlias then identifies C2, while calls through the pre-existing alias interface still use C1.
Clearing A leaves that old interface usable.
New aliases added through SetAlias or CreateCollection lack an exported alias path during the invocation; CreateCollection registers only the collection path (`crates/underwrap-secretsd/src/dbus_service/service.rs:86-97`).

The fixed ID controls collection reads, creation, label changes and deletion (`crates/underwrap-secretsd/src/dbus_service/collection.rs:52-63,70-81,105-144`).
This is not merely a stale display label.

## Consequence and boundary

A well-behaved client using an alias can read from, create items in, relabel or delete the wrong collection after a successful alias change.
The published graph and callable D-Bus object graph disagree.
The alias/reference grammar remains valid and the host publishes exactly the candidate it accepted; the defect lies in exported routing.

The affected asset is integrity of this invocation's libsecret routing/state.
Security treats target D-Bus input as robustness-untrusted (`docs/security.md:14-17`); accepted alias semantics are described in `docs/local-protocol.md:219-221,249-253`.
This does not establish access to another resource or caller-to-secret disclosure.
Full Secret Service draft compatibility, independent per-client sessions and a hostile-target sandbox remain outside the chosen model.

## Correction and alternatives

Resolve alias operations from the currently accepted map or update exported objects coherently after accepted mutations.
No strategy is selected; whichever is chosen must cover alias creation, retargeting, clearing and collection deletion.
Keep strict identifier/reference validation, store locking, ACK-before-commit and rollback after rejection.
ACK does not by itself make object registration and response delivery atomic.

The stale deleted-object success issue is separate in [RF-0007](../RF-0007/README.md), though object-lifetime corrections may overlap.

## Acceptance criteria

- Through an actual exported alias path, retarget A from C1 to C2 with ACK and verify reads/mutations affect only C2; C1 remains unchanged.
- After accepted clear, old alias-path operations must not continue accessing C1.
- Aliases created by either supported method must expose usable paths consistent with ReadAlias.
- Accepted collection deletion must remove or invalidate dependent alias routing; rejected changes must preserve previous map and callable behavior.
- Include ordinary startup alias and rejected-publication controls.
  Correction verification and its limits are recorded in [Evidence](evidence.md#rf-0006-acceptance).

## Work ownership

This file owns the user-authorized joint RF-0006/RF-0007 correction from local main `7260be4feff5c2bc70d958f50e50adf218cad994`.
Scope is accepted Secret Service routing and exported object lifetimes, not identifier grammar, storage, runtime DTOs, namespace diagnostics or other findings.
Post-ACK export reconciliation failure terminates the invocation-private helper without retracting the acknowledged generation.

- [x] Resolve aliases from accepted state under the store lock and serialize graph methods through reconciliation without retaining the store lock during registry writes.
- [x] Reject missing-object mutations under the store lock, including retained handlers, and remove deleted descendants and aliases.
- [x] Exercise actual D-Bus dispatch for create/retarget/clear/delete, rejected/transport controls, and bounded registry reuse on peer connections and a private bus.
- [x] Verify post-ACK registration failure and actual pending-reconciliation cancellation stop isolated test processes; retain candidate wiping, publication ordering and deterministic property/registry overlap controls.
- [x] Run relevant Secret Service/store/protocol tests, formatting, denied-warning workspace Clippy, the full integration runner and final-source shipped-chain VM check; retain separate acceptance and evidence limits for each finding.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
