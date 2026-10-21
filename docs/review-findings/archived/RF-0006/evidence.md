# RF-0006 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `S-01`.
Original review owners: `secret-service.md`, `substantive-inputs.md`, `coordinator-notes.md`, `documentation-replacement.md`, `report.md`, `input-ledger.md`, `claim-evidence-join.md`, `synthesis-verification.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Startup copy: `crates/underwrap-secretsd/src/main.rs:220-230`; map-only SetAlias: `crates/underwrap-secretsd/src/dbus_service/service.rs:198-232`; CreateCollection registration: `crates/underwrap-secretsd/src/dbus_service/service.rs:86-97`; fixed-ID operations: `crates/underwrap-secretsd/src/dbus_service/collection.rs:52-63,70-81,105-144`.

## Recorded observations

The Secret Service reviewer followed the complete implementation, and the coordinator separately confirmed that accepted SetAlias does not update/remove/register alias interfaces.
Independent synthesis retained this as source-confirmed, not wire-executed.

Inspected `crates/underwrap-secretsd/src/dbus_service/service/tests.rs:85-110,192-232` checks accepted store/ReadAlias/publication state only.
Registration tests at `crates/underwrap-secretsd/src/dbus_service/service/tests.rs:235-308` inspect collection paths, not alias paths.
Those assertions do not discriminate stale exported routing.
The shared mutation path at `crates/underwrap-secretsd/src/dbus_service/mutation.rs:10-20` still provides candidate publication and rollback.

## Reproduction or verification procedure

The recorded evidence is source inspection, not execution.
A future normalized wire verification requires no historical probe:

1. Start a disposable private bus and current secretsd with two valid synthetic collections and one valid startup alias.
   Use empty/nonsecret values and a real host publication receiver or a clearly identified matching-ACK test peer.
2. Establish baseline ReadAlias and direct alias-path collection operations.
   Retarget via SetAlias, wait for accepted publication, then compare ReadAlias with reads and a harmless mutation made through the alias path.
   Inspect which collection changed without logging material.
3. Repeat for clear, a newly added SetAlias alias, CreateCollection-with-alias and collection deletion.
   Inspect exported dispatch, not just the in-memory alias map.
4. Reject a publication attempt and verify both accepted state and callable routing remain unchanged.
   Stop/reap disposable bus/helper processes.
   A simulated ACK peer is not evidence of full namespace readiness or persistence.

## Limits and missing evidence

No fresh private-bus wire call, dispatch/registration regression or end-to-end persistence case was executed for this finding.
Existing suite passes do not close those missing assertions.
No cross-resource leak, malformed-publication acceptance, per-client isolation failure or complete D-Bus conformance conclusion follows.
No new source review or execution occurred during migration.

## Resolution evidence

Fixed in `80b904ca0167ad9c2c87183045cf1ff60feebd01`, based on local main `7260be4feff5c2bc70d958f50e50adf218cad994`.
Verification used the Nix development shell on x86_64 Linux with pinned Rust 1.97.1 and a disposable dbus-daemon 1.16.2 bus.
The final source was checked before committing; no runtime or test input changes follow in the archival documentation commit.

### RF-0006 acceptance

`crates/underwrap-secretsd/src/dbus_service/wire_tests.rs::aliases_follow_accepted_state_on_wire` exercises startup alias reads, ACKed C1-to-C2 retarget, collection Label and item creation through that same exported alias, and complete unchanged C1 state.
It checks accepted clear removes dispatch and retained alias setters return UnknownObject, both creation methods expose callable paths matching ReadAlias, and deletion through a subsequently retargeted alias deletes only its current collection and cleans dependent aliases.
`wire_tests/rollback.rs` checks rejected retarget/clear/create/deletion and EOF/wrong-generation responses preserve complete accepted state and callable routing.
These execute actual D-Bus messages, not only typed handlers, with simulated publication responses.

### Shared required checks

All commands below exited 0 on the final implementation inputs.
Run Cargo commands inside `nix develop`; the private-bus command requires dbus-daemon on PATH.

| Command | Observed result and scope |
| --- | --- |
| `cargo test --locked -p underwrap-secretsd -p underwrap-secretsd-store -p underwrap-protocol` | 67 secretsd, 23 store and 126 protocol tests passed, retaining identifier, strict validation, candidate wiping and publication controls. |
| `UNDERWRAP_TEST_DBUS_DAEMON=$(command -v dbus-daemon) cargo test --locked -p underwrap-secretsd dbus_service::wire_tests` | All seven witnesses passed on a real private bus; default crate execution also passed them over pre-authenticated peer sockets. |
| `UNDERWRAP_DEBUG_SECRETSD_TRACE_METADATA=1 cargo test --locked -p underwrap-secretsd property_validation` | All five RF-0003 property/prevalidation and active tracing controls passed. |
| `cargo test --locked -p underwrap-ns publication` | 17 host publication/readiness/terminal-selection controls passed, including rejection, partial ACK, response loss, retained endpoints and composition limits. |
| `cargo fmt -- --check` | Passed. |
| `UNDERWRAP_WEB_DIST=<built web-build output> cargo clippy --locked --all-targets --workspace -- -D warnings` | Passed with the real SPA artifact produced by `nix build --no-link --print-out-paths .#checks.x86_64-linux.web-build`. |
| `./crates/integration-tests/run` | 114 real daemon/backend integration tests passed; reported test execution was 22.14 seconds, separate from prerequisite builds. |
| `nix build --no-link .#checks.x86_64-linux.vm-test` | Passed on the final source, including the shipped-chain files, libsecret startup/lookup and RF-0001/RF-0003 controls; VM scenarios were unchanged. |
| Documentation validation and `git diff --check` | Local links, anchors, Markdown fences, finding layout/index consistency and whitespace checked for the changed topic/finding documents. |

The joint source review followed secretsd candidate ownership and ACK handling into the unchanged namespace receiver's strict decoding, composition check, ACK-written selection and terminal EOF rules, plus shared protocol graph/reference validation.
Independent read-only review identified the registry/store lock inversion in the initial design; the corrected graph gate releases the store before registry writes and uses shared-reference property dispatch.
`wire_tests/concurrency.rs` now deterministically polls a second graph method into gate contention and overlaps actual Properties.Get with pending reconciliation.
Isolated child tests exercise an unexpected registration collision after ACK and cancellation of a genuinely pending post-ACK reconciliation; each terminates with exit 1 and the fixed value-free diagnostic.
Initial real-bus fixture calls stalled before dispatcher subscription was ready; builder `serve_at` now establishes dispatcher readiness in the fixture and production startup rather than relying on scheduling.

### Limits

The private-bus ACK/REJECT peer is simulated host retention, not backend durability, namespace publication receipt or D-Bus caller receipt.
The VM and HTTPS integration suites provide their existing deployed/persistence witnesses, not a new alias-roundtrip backend-durability scenario or complete D-Bus conformance.
There is no atomic registry transaction, universal availability, complete erasure or recovery claim.
The full aggregate `nix flake check` was not rerun: this Rust/Secret Service change used the relevant testing-matrix gates above and does not resolve or waive RF-0012's separately recorded aggregate SPA blocker.
The discovery evidence above remains revision-scoped and is not rewritten as executed reproduction.
