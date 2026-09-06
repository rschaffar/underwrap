# Independent security-review handoff

Date: 2026-09-01.

Status: Bounded fresh-context internal review and correction verification complete.

## Candidate and requested review

The reviewed product candidate is commit `5aae8810d1e941e214e4494057d8c750bcc0f0e4`.

At validation time it was the tip of branch `release/readiness` and the required rebase of validated candidate `f61d8ddeb76de76dad0e4c8bffc45dcee7c0bd9e` onto local `main` commit `7b3145615f5cd775aa2280c4d07315e47d66e066`.

Only `AGENTS.md` workflow guidance differs between those product-candidate trees, and the exact rebased candidate received a fresh direct matrix.

The evidence and handoff additions produced feature tip `402060e73804f5ea4f64755258123bccdd5ea542` without changing product behavior.

Local `main` integrated that feature tip through merge `8ce345c333a91ce937cf39dd85124c2dd772b8ae`.

The feature branch and linked worktree were then removed, while both candidate commits remain reachable through the merge.

The requested review was an independent assessment, not authorization to release, tag, publish, weaken a boundary, or change a protocol or dependency.

The review covered correctness, security boundaries, evidence, documentation, and overclaim risks.

## Review outcome

The [bounded independent review record](2026-09-01_release-readiness-independent-review.md) reports no Critical or Major product finding and two Minor corrections.

One correction extends backend-fixture startup coordination across cooperating test processes and preserves the explicit unrelated-process limitation.

The other reconciles this project memory with the completed merge and cleanup.

A second fresh-context bounded internal verification accepted exact correction range `8ce345c333a91ce937cf39dd85124c2dd772b8ae..2d24142462fb8589ccb23246b39c16e0684e703f`, classified both Minor findings as fixed, and reported no unresolved correction defect at any severity.

Correction documentation and verification produced tip `abc94fc0ec0842272d0890702bd1be097e2f8aaf`, which local `main` integrated through final merge `dfe0ca7429e9c5e0ac7568d35eab3675257527f1` before the correction branch and worktree were removed.

The completed plan is archived at [`2026-09-01_release-readiness-plan.md`](../plans/archived/2026-09-01_release-readiness-plan.md).

This review and verification are internal rather than an external independent end-to-end security audit.

## Start here

Read these current owners in order:

1. [Architecture](../architecture.md) for components, identities, and request flow.
2. [Security](../security.md) for the threat model, guarantees, mechanisms, and limitations.
3. [secretsd security](../secretsd-security.md) for the target-facing D-Bus input boundary.
4. [Backend](../backend.md) for runtime/admin APIs, policy, entities, and storage.
5. [Configuration](../configuration.md) for the NixOS and backend configuration surfaces.
6. [Testing](../testing.md) for validation layers and explicit nonclaims.
7. [Release-readiness evidence](2026-09-01_release-readiness-evidence.md) for fresh candidate results and unavailable properties.
8. [Workspace validation stability](2026-09-01_workspace-validation-stability.md), [VM equality investigation](2026-09-01_vm-raw-blob-equality.md), and [SPA coverage investigation](2026-09-01_spa-function-coverage.md) for the release-gate corrections.

Accepted decisions are indexed in the [ADR directory](../adr/README.md).

ADRs 0002 through 0006 are especially relevant to privileged configuration grammar, namespace-local protocol profile, error redaction, observability, and Rust support.

ADR 0008 owns the SPA coverage gate and its nonclaims.

## Supported scope

The backend supports Linux.

The privileged frontend wrapper chain is supported through the NixOS module.

The flake exposes x86-64 and AArch64 Linux packages and checks.

Fresh runtime evidence in this handoff is x86-64 only.

The supported Rust baseline and resolver are owned by [ADR-0006](../adr/0006_rust-support-and-resolver.md).

The frontend tool matrix is owned by [ADR-0007](../adr/0007_frontend-tool-matrix.md).

The candidate remains pre-release and has not received an independent end-to-end security audit.

## Protected assets, attackers, and trust

The primary confidentiality asset is brokered resource material projected to a configured target.

The caller, including another same-caller-UID process, is untrusted with respect to that material.

Dedicated target UIDs and the wrapper namespace are the boundary from caller processes toward the target projection.

The configured target is trusted with its own projection and can print, persist, or transmit it.

Target-supplied Secret Service messages remain untrusted for parser robustness and storage integrity.

The backend is the authorization and persistence authority.

A runtime client instance is authenticated by its enrolled mTLS certificate fingerprint and is not a local Unix-user principal.

Root-owned NixOS configuration, backend administration, host root, the kernel, and trusted operator actions are assumptions rather than defended attackers.

The exact normative limits are in [Security](../security.md); use that document rather than broadening these summaries into new claims.

## High-value call paths

### Local invocation identity and admission

Review:

- [`nix/modules/underwrap.nix`](../../nix/modules/underwrap.nix) for accounts, setgid entry, sidecars, target UID table, capabilities, services, and hardened-host assertions;
- [`crates/underwrap-exec/src/main.rs`](../../crates/underwrap-exec/src/main.rs) for instance selection, daemon request, privilege drop, fd relocation, and stage handoff;
- [`crates/underwrapd/src/peer_auth.rs`](../../crates/underwrapd/src/peer_auth.rs) for `SO_PEERPIDFD`, effective-GID authentication, and explicit fallback posture;
- [`crates/underwrapd/src/server.rs`](../../crates/underwrapd/src/server.rs) and [`wrapper_flow/`](../../crates/underwrapd/src/wrapper_flow/) for wrapper-name validation and backend-authorized target selection;
- [`crates/underwrap-ns/src/handoff.rs`](../../crates/underwrap-ns/src/handoff.rs) for fixed-fd daemon authentication.

Confirm that a client-supplied wrapper name remains a selector rather than proof of identity and that no fallback executes as caller or root.

### Secret-bearing open and channel path

Review these files together:

- [`crates/underwrapd/src/backend_client.rs`](../../crates/underwrapd/src/backend_client.rs);
- [`crates/underwrapd/src/wrapper_flow/read.rs`](../../crates/underwrapd/src/wrapper_flow/read.rs);
- [`crates/underwrapd/src/wrapper_resource_channel.rs`](../../crates/underwrapd/src/wrapper_resource_channel.rs);
- [`crates/underwrap-protocol/src/message/wrapper_resource.rs`](../../crates/underwrap-protocol/src/message/wrapper_resource.rs);
- [`crates/underwrap-ns/src/resource_channel.rs`](../../crates/underwrap-ns/src/resource_channel.rs).

Confirm complete bounded decoding, conditional env-name validation, original-byte cache lifetime, fd ownership, no secret-bearing diagnostics, and zeroization where practical.

### Namespace and target execution

Review:

- [`crates/underwrap-ns/src/main.rs`](../../crates/underwrap-ns/src/main.rs);
- [`target_uid.rs`](../../crates/underwrap-ns/src/target_uid.rs);
- [`env.rs`](../../crates/underwrap-ns/src/env.rs);
- [`mounts.rs`](../../crates/underwrap-ns/src/mounts.rs);
- [`directory_credential.rs`](../../crates/underwrap-ns/src/directory_credential.rs);
- [`libsecret.rs`](../../crates/underwrap-ns/src/libsecret.rs);
- [`exec_gate.rs`](../../crates/underwrap-ns/src/exec_gate.rs);
- [`target_status.rs`](../../crates/underwrap-ns/src/target_status.rs).

Confirm root-owned target identity selection, permanent ID drop, group and capability clearing, no-new-privileges, dump/core controls, environment clearing, fd closure, helper readiness, and fail-closed lifecycle conjunctions.

### Policy, persistence, and writeback

Review:

- [`crates/underwrap-protocol/src/policy.rs`](../../crates/underwrap-protocol/src/policy.rs);
- [`crates/underwrap-backend/src/runtime_api/invocations.rs`](../../crates/underwrap-backend/src/runtime_api/invocations.rs);
- [`crates/underwrap-ns/src/writeback.rs`](../../crates/underwrap-ns/src/writeback.rs);
- [`crates/underwrap-backend-store/src/plain.rs`](../../crates/underwrap-backend-store/src/plain.rs);
- [`crates/underwrap-backend-store/src/encrypted.rs`](../../crates/underwrap-backend-store/src/encrypted.rs);
- [`crates/underwrap-backend-store/src/resource_blob.rs`](../../crates/underwrap-backend-store/src/resource_blob.rs).

Confirm permission re-evaluation on open and finalize, writable re-check, immutable env comparison, complete-resource size and canonicalization rules, atomic persistent replacement, and the documented last-writer-wins limitation.

### Secret Service publication

Review:

- [`crates/underwrap-secretsd/src/publication.rs`](../../crates/underwrap-secretsd/src/publication.rs);
- [`crates/underwrap-ns/src/publication.rs`](../../crates/underwrap-ns/src/publication.rs);
- [`crates/underwrap-protocol/src/resource_blob.rs`](../../crates/underwrap-protocol/src/resource_blob.rs);
- [`crates/underwrap-secretsd-store/`](../../crates/underwrap-secretsd-store/).

Confirm strict publication validation, generation/ack behavior, in-memory state, mutation rollback, and no alternate durable frontend resource path.

### Backend administration and transport

Review:

- [`crates/underwrap-backend/src/server.rs`](../../crates/underwrap-backend/src/server.rs);
- [`auth.rs`](../../crates/underwrap-backend/src/auth.rs);
- [`admin_api/`](../../crates/underwrap-backend/src/admin_api/);
- [`runtime_api/`](../../crates/underwrap-backend/src/runtime_api/);
- [`runtime_clients.rs`](../../crates/underwrap-backend/src/runtime_clients.rs);
- [`storage/`](../../crates/underwrap-backend/src/storage/).

Confirm backend-owned rustls mTLS principal derivation, session and CSRF enforcement, offline bootstrap/recovery boundaries, internal-error redaction, registry atomicity, encrypted-key lifetime, and store lock/rotation behavior.

## Release-readiness changes needing focused attention

Review the release range for four evidence-boundary corrections:

- bounded exact VM blob transfer retained byte-for-byte F-007/F-009 assertions while avoiding one monolithic multi-megabyte test-driver response;
- meaningful SPA endpoint tests raised function coverage without threshold or exclusion changes, and Nix now enforces the selected gate;
- integration backend fixtures serialize the selected port's release-to-bind window through HTTPS readiness rather than retrying or reducing concurrency;
- fd and EOF witnesses run in single-test child processes where process-global descriptor reuse or inheritance cannot falsify their assertions.

The restrictive-umask rotation test retains `umask 0700` during the operation and changes only its throwaway child's exit umask so LLVM can read the generated profile.

Confirm that these are test-evidence changes and do not alter production behavior or weaken the asserted properties.

The initial backend-fixture guard was process-local.

Review correction `efecdb1373fdd04e5f5600f5ebc31ba49670fc13` adds a close-on-exec advisory file lock for cooperating Cargo and nextest processes while explicitly retaining the unrelated-process limit.

## Build, dependencies, and generated contracts

The root [`Cargo.lock`](../../Cargo.lock), [`flake.lock`](../../flake.lock), and [`web/package-lock.json`](../../web/package-lock.json) are the pinned dependency inputs.

The committed [`crates/underwrap-backend/openapi.json`](../../crates/underwrap-backend/openapi.json) is the normative admin HTTP contract.

SPA test and build commands regenerate `web/src/api/schema.ts` from that document.

The flake builds and embeds the SPA into the backend package.

Use the validation commands in [Testing](../testing.md).

For the exact release matrix and output paths, use the dated [evidence record](2026-09-01_release-readiness-evidence.md).

## Review questions

At minimum, answer these questions with source evidence:

1. Does each documented confidentiality and authorization claim have a matching fail-closed implementation path?
2. Can an untrusted caller bypass the setgid/effective-GID/fixed-fd admission sequence or influence target identity or executable selection?
3. Can secret material reach argv, inter-stage environment, ordinary disk, logs, diagnostics, frontend caches, or inherited descriptors outside the documented projection paths?
4. Do malformed, oversized, noncanonical, partial, duplicated, or reordered protocol inputs fail before unsafe effects?
5. Are policy and writable checks repeated at every required open/finalize boundary without an undocumented compare-and-swap claim?
6. Do encrypted-store lifecycle, registry persistence, and offline recovery retain their documented atomicity and key-lifetime boundaries?
7. Do the NixOS module assertions and service/wrapper construction actually establish the security posture claimed by the prose?
8. Does the candidate evidence support only the claims made, with direct, simulated, unavailable, and historical results kept distinct?

## Known limitations and non-scope

The disposable walkthrough is same-host loopback evidence and does not cover DNS, firewall traversal, cross-host routing, latency, partitions, multi-machine orchestration, or distinct-host compromise.

AArch64 outputs were evaluated but not built or run.

The browser suite and deployment walkthrough are separate evidence layers rather than one combined browser-through-VM flow.

Coverage is a regression floor, not proof of correctness or security.

The lower-level reason for one historical short monolithic NixOS driver response remains unknown after the guest bytes were proven unchanged.

LCOV execution-count output was not proven byte-reproducible under a diagnostic rebuild.

Configured targets can disclose their own projections, and host root or kernel compromise remains out of scope.

No production system, credential, release tag, package publication, or version bump is in scope.

Independent acceptance was pending when this handoff was originally issued.

The later bounded internal review outcome, correction verification, and external-audit nonclaim are recorded in the linked review record.
