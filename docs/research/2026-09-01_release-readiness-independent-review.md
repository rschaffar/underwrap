# Release-readiness independent review

Date: 2026-09-01.

Status: Two minor findings corrected and independently verified.

## Scope and classification

A fresh-context reviewer in the development agent harness performed a bounded read-only review of `7b3145615f5cd775aa2280c4d07315e47d66e066..402060e73804f5ea4f64755258123bccdd5ea542`.

The review covered product candidate `5aae8810d1e941e214e4494057d8c750bcc0f0e4`, the later evidence and handoff records, and the integrated tree at merge `8ce345c333a91ce937cf39dd85124c2dd772b8ae`.

The reviewer inspected every release-readiness change, the affected call paths, and the high-value security paths named by the handoff.

This was an independent fresh-context internal review, not an external end-to-end audit, formal verification, or release authorization.

The reviewer reached its configured turn limit while wrapping up after emitting a complete findings report.

## Verdict

The bounded verdict was accept with minor corrections.

No Critical or Major product security or correctness finding was confirmed.

The reviewer found no bypass of the effective-GID, wrapper-selector, fixed-fd, target-identity, backend-policy, writable, namespace, projection, persistence, or NixOS-posture boundaries inspected within scope.

The reviewer found no release-readiness regression that placed brokered resource material in argv, inter-stage environment, ordinary disk, logs, or unintended inherited descriptors.

## Confirmed findings

### Cooperative backend startup was process-local

The integration harness released its probe listener while holding only a process-local mutex through HTTPS readiness.

That excluded cooperating fixtures under ordinary `cargo test`, but separate nextest processes or another cooperating test process could still select the same released port.

The affected asset was validation reliability rather than product confidentiality or authorization.

An unrelated process that does not participate in an advisory coordination mechanism can still claim a released loopback port.

### Review records lagged repository topology

The release-readiness work had already been merged into local `main` at `8ce345c333a91ce937cf39dd85124c2dd772b8ae` and its feature branch and worktree had been removed.

The handoff, evidence record, and active plan still described the deleted branch, independent review, integration, and cleanup as current pending state.

This affected current project-memory accuracy and reviewer navigation rather than product behavior.

## Corrections

Commit `efecdb1373fdd04e5f5600f5ebc31ba49670fc13` adds a close-on-exec advisory file lock around the integration harness's release-to-bind window while retaining its asynchronous process-local mutex.

The file lock serializes cooperating Cargo and nextest processes that share the same temporary directory and effective UID until the selected backend answers the HTTPS readiness probe.

A fresh single-test child proves that a second process cannot acquire the lock, that dropping the reservation releases it, and that backend child execs cannot inherit its descriptor.

The helper and documentation describe the remaining unrelated-process limit instead of claiming universal port ownership after the probe listener is released.

The current-state documentation records the merge, removed review branch, completed bounded review, corrections, and still-pending external-audit and archival boundaries without erasing the original dated evidence.

No production crate, dependency, protocol, public API, privileged binary chain, or security boundary changed.

## Direct correction evidence

The focused cross-process reservation test passed.

Rustfmt and denied-warning Clippy for `integration-tests` passed.

All 107 integration tests passed with log SHA-256 `422a80ae63311d8db13cc0f985227944a992c58fc934f4fbe622f3e1495635e4`.

The Nix `workspace-nextest` check passed with local build log SHA-256 `c411b838d0660faeeaea322e34c45977dad3ccf5676d10f17d38bd46eaace2e3`.

These local logs are temporary validation artifacts rather than committed evidence.

## Follow-up verification

A second fresh-context reviewer performed a bounded read-only verification of exact correction range `8ce345c333a91ce937cf39dd85124c2dd772b8ae..2d24142462fb8589ccb23246b39c16e0684e703f`.

It classified both original Minor findings as fixed, found no Critical, Major, or Minor correction defect, and accepted the range.

It directly passed the focused cross-process reservation test, Rustfmt, denied-warning integration-test Clippy, diff checks, documentation links and anchors, the one-sentence-per-line heuristic, and a bounded sensitive-pattern scan.

It confirmed that every relevant release-to-bind backend fixture retains both locks through HTTPS readiness and that the records preserve the unrelated-process, external-audit, final-acceptance, and archival limits.

The shared CodeGraph index failed to synchronize the linked worktree with status 1, so the reviewer used exact diff and source inspection without creating another index.

An initial verifier command started from the primary checkout and stopped before testing because its SPA artifact was absent, so it contributed no acceptance evidence.

Separately, exact reviewed tip `2d24142462fb8589ccb23246b39c16e0684e703f` passed all-system flake evaluation without builds and all 125 compatible current-system flake checks, including `workspace-nextest`, LLVM coverage, and the NixOS VM.

Those aggregate logs have SHA-256 values `f4e98a251308ebf4f020170989460a83854097d6b36b00506891bb6be0ae5d59` and `14055002de672f16ee2137672cef67d36d75c5d31393d440d32eaa57dd592e26` respectively.

The aggregate run omitted AArch64 execution as incompatible with the current x86_64-linux host.

This follow-up remains a bounded internal verification rather than an external independent end-to-end audit.

## Review checks and unavailable evidence

The independent reviewer directly ran Python compilation for the VM helper, all 30 SPA endpoint cases, the complete 192-test SPA coverage command, and focused Rust tests for every release-readiness isolation correction and backend startup reservation.

The reviewer did not rerun the complete Cargo/Nix/Playwright matrix, the NixOS VM, or the disposable deployment walkthrough.

The original temporary matrix logs had already been removed and their recorded hashes could not be independently recomputed from those files.

AArch64 runtime, separate-host networking, kernel failure behavior, power-loss durability, formal verification, and external end-to-end audit remain unavailable.
