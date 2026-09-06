# VM raw-blob equality investigation

Status: Cause isolated and corrected-revision pass streak complete.

Date: 2026-09-01.

Completed plan: [`2026-09-01_release-readiness-plan.md`](../plans/archived/2026-09-01_release-readiness-plan.md).

## Scope

This investigation covers the intermittent F-007 and F-009 NixOS VM equality failures previously retained in the VM raw-blob equality roadmap.

The protected asset is the integrity of backend-managed canonical resource bytes across target completion, namespace teardown, daemon finalization, backend persistence, and test observation.

The investigation did not authorize or make a product, protocol, storage-format, wrapper-chain, or security-boundary change.

## Historical input

Exact-tree runs during F-032 intermittently stopped at the existing F-007 and F-009 comparisons after the relevant wrapper path had otherwise reached its expected terminal state.

A clean detached checkout reproduced one F-007 failure without the F-032 daemon change.

A later F-052 run stopped once at an F-009 comparison and passed completely on an immediate same-tree rerun.

A later F-054 run stopped once at an F-007 comparison and passed completely on an immediate unchanged-tree rerun, while the subsequent F-055 VM run passed on its first attempt.

On 2026-09-01 the user transferred this unresolved evidence from the completed F-031 through F-058 remediation program into a roadmap without claiming a cause or resolution.

The release-readiness plan then promoted that direction into this investigation.

## Source trace

`crates/underwrap-ns/src/main.rs::run_parent` waits for the namespace relay, requires a clean relay status, reads the direct target status, drains terminal publication state, and only then calls `dispatch_after_gate`.

A cancellation or unclean relay reaches `parent_supervisor_failure`, closes the resource channel, and does not dispatch writeback.

`crates/underwrap-ns/src/writeback.rs::dispatch_one` canonicalizes the complete post-run resource and compares those bytes directly with the Checkout bytes.

Equal bytes send `UsageSignal`, changed writable bytes synchronously send `Checkin`, and an oversized complete resource returns `RejectedTooLarge` without a terminal resource verb.

`crates/underwrapd/src/wrapper_resource_channel.rs::handle_checkin` awaits backend finalization before returning `CheckinOk`, while `handle_usage_signal` finalizes with no blob.

`crates/underwrap-backend/src/runtime_api/invocations.rs::handle_runtime_wrapper_invocation_finalize` rechecks policy and writability, decodes and canonicalizes a submitted blob, checks env immutability, saves it, and only then returns `Ok`.

A no-blob finalize does not save resource bytes and only updates pair usage metadata.

`crates/underwrap-backend/src/storage/secret_entries.rs::write_resource_entry` composes an admin mutation in memory before calling `save_resource_blob`.

`crates/underwrap-backend-store/src/resource_blob.rs::save_resource_blob` canonicalizes before calling the selected storage backend, so an oversized composition fails before a store write.

`crates/underwrap-backend-store/src/plain.rs::PlainBackend::write` delegates to `atomic_write`, and `crates/underwrap-backend-store/src/path.rs::atomic_write` uses a same-directory `NamedTempFile` followed by `persist()`.

The original `nix/tests/test.py::admin_get_blob` instead read the PlainBackend file with one guest-side `base64 -w0` command and returned the complete shell response through the NixOS test driver.

The pinned NixOS test driver 1.1 implementation wraps command output in another unbroken base64 stream over its virtio console before decoding it in the host-side driver.

## Reproduction evidence

The first uninstrumented run at commit `5582596` failed the F-007 equality for the `host-kill` case after the preceding `stage1-kill` case passed.

At investigation time, its full log was retained outside the repository at `/tmp/underwrap-release-readiness-vm-run-01.log`, with SHA-256 `1bb223fc0815fe2d742c3936de0fb65a24f356c31661dcbe6515f84bb9351503` and size 569145 bytes.

That temporary log was later removed during final candidate hygiene.

A second run with F-007 length-and-digest diagnostics passed F-007 and failed the F-009 equality after the rejected oversized admin mutation.

Its full log had SHA-256 `637725bda184e482d443bf99a85ca29434d0ccc73a6debedb1ebc889e4bfc45b` and size 709254 bytes.

A third run with the same observations extended through F-009 passed the earlier equalities and then decoded a short driver observation after the valid same-length replacement.

Its full log had SHA-256 `55cd5d23beff7a08e12314bce40367e2e0ef715ca2324a041b698f2786b5a153` and size 714596 bytes.

Two subsequent diagnostic runs passed completely.

The next forced diagnostic run failed the final F-009 readback equality while the guest-side file remained 2097152 bytes with the same SHA-256 before and after the readback.

The expected driver response contained 2796204 base64 characters, while the observed response contained 2565804 characters.

Its full log had SHA-256 `01d00c6ed2c4b6212b7621abfabb72e2aa233c1c8b6fd7915c2e0df1eb4a3abb` and size 717936 bytes.

No resource bytes, fixture values, credentials, canaries, or key material were added to the diagnostics or this record.

## Hypotheses

| Candidate | Disposition | Evidence |
|---|---|---|
| Cancellation or unchanged execution dispatched an unintended product writeback | Rejected | The source trace suppresses dispatch after unclean relay teardown and sends only `UsageSignal` for equal canonical bytes, and the decisive F-009 failure retained the same guest-side file fingerprint. |
| The rejected oversized admin mutation partially persisted | Rejected | Canonicalization fails before `StorageBackend::write`, PlainBackend replacement is atomic, and later failures occurred on unchanged readback with an unchanged guest-side fingerprint. |
| PlainBackend readers observed a partial same-file write | Rejected | Writes use same-directory atomic replacement, and the decisive failure reported identical guest-side length and digest before and after observation. |
| Canonical resource assembly was nondeterministic | Rejected for the reproduced mismatch | The decisive failure compared two observations of one unchanged 2097152-byte guest file and isolated a shorter driver response rather than different stored bytes. |
| One monolithic test-driver shell response was truncated | Supported and isolated | The decisive run retained an identical guest-side fingerprint while losing 230400 base64 characters only in the driver-returned observation. |
| The precise lower-layer reason that the virtio-console response loses bytes | Still unknown | The evidence isolates the harness transport boundary but does not distinguish a NixOS test-driver, QEMU virtio-console, buffering, or host scheduling defect below that boundary. |

## Correction

`nix/tests/test.py::admin_get_blob` now transfers the exact file in independent 64-KiB chunks instead of one multi-megabyte shell response.

Each chunk uses `machine.execute` so a command failure cannot cause the test driver to log secret-bearing output automatically.

The host validates each chunk as base64, checks its exact expected byte length, joins the chunks, and verifies the complete length and SHA-256 against guest-side fingerprints taken before and after the observation.

A file change during observation fails immediately and is not retried.

The existing byte-for-byte F-007 and F-009 equality assertions and their expected values remain unchanged.

The correction affects only VM-test observation and does not change runtime behavior or weaken the canonical-byte witness.

The correction was committed as `49a0d310ad99aa0641f2f01987031f87c05aeff6`.

Five consecutive `nix build --rebuild --no-link --print-out-paths .#checks.x86_64-linux.vm-test` executions passed on that exact clean revision and the same `/nix/store/m2pn19l2jyilzpw9r74mxixsibhrcqr8-vm-test-run-underwrap-vm-test.drv` derivation.

| Forced run | Result | Full-log SHA-256 | Full-log size |
|---|---|---|---|
| 1 | `ALL TESTS PASSED` | `863d454518bf916c5c0568b3a97c7b69c550ce3fcb7a75effd3c82ebb164100f` | 976115 bytes |
| 2 | `ALL TESTS PASSED` | `9474ec4d575fdebe6584ac5cce66d4dd3c0c96f79949072151f44eeafaf682c7` | 975937 bytes |
| 3 | `ALL TESTS PASSED` | `ca3d6d9df9366050e922b577dd697ad6abcc16b51ddd18ee0f3f6522c1f1bcf0` | 975289 bytes |
| 4 | `ALL TESTS PASSED` | `d876b30ca6cb63781708b360eb7428eebc59a1b36971359d586e9549c9d7411d` | 976086 bytes |
| 5 | `ALL TESTS PASSED` | `3bd465b1d9d4eef562a7ecc20358162b2daed3e3ef1b05a418746dd7170d6d83` | 975666 bytes |

At validation time, the full logs were retained outside the repository under `/tmp/underwrap-release-readiness-vm-validated-0N-full.log`.

Those temporary logs were later removed during final candidate hygiene; the table retains their recorded hashes and sizes.

## Limits

The decisive diagnostic proves that the reproduced mismatch was in the observation transport rather than the guest-side backend file.

It does not prove the detailed lower-layer truncation mechanism or mathematically exclude a different future defect that could also trip the retained equality assertions.

Repeated passing VM runs remain bounded empirical evidence rather than proof that every scheduling interleaving is safe.
