# Complete the accepted codebase-review remediation

Status: Active

Execution authority: On 2026-08-27 the user explicitly selected **Transfer and continue**, carrying D-174's standing authority into this plan through F-058 with every stop condition and severity gate intact.

## Goal

Resolve every remaining accepted finding from F-031 through F-058 in severity order while preserving the review program's security boundaries, cleanup-first discipline, independent review, validation, and durable evidence.

## Context

The accepted report is [`tickets/_review/codebase_2026-07-18.md`](../../tickets/_review/codebase_2026-07-18.md).

The historical program state and decisions are in [`tickets/_handover/codebase_2026-07-18.md`](../../tickets/_handover/codebase_2026-07-18.md).

Completed tickets F-001 through F-030 remain immutable evidence under [`tickets/03_done/`](../../tickets/03_done/).

The report, handover, and completed tickets are evidence sources rather than active work owners.

This plan is the single active owner for the remaining accepted review program.

## Plan Relationship

This plan replaces the handover's future ticket-materialization instructions without rewriting the historical records.

No per-finding ticket or competing per-finding plan is created.

Each finding is still executed as one active, coherent iteration at a time.

New comprehensive reviews may append new reports under `tickets/_review/`, but they do not alter this plan unless their findings are explicitly accepted and assigned here.

Opportunities O-001 through O-026 and unresolved items U-001 through U-011 remain outside this plan unless the user separately approves the required behavior or evidence work.

## Security Invariants

For every security-relevant iteration, record the protected asset, attacker or failure source, boundary direction, and preserved invariant from `docs/security.md`.

Do not add a dependency, change the `underwrap-exec` → `underwrap-ns` binary chain, weaken a security boundary, or absorb an Opportunity or unresolved item without the required focused user decision.

Preserve the caller-untrusted, configured-target-trusted, target-protocol-untrusted, backend-policy-authority, dedicated-target-UID, fixed-fd-20, descriptor-free-fd-3, fail-closed, and writeback-revalidation boundaries.

Never expose secret values in plans, prompts, diffs, tests, logs, review artifacts, or commits.

## Assumptions

D-201 selects the strict F-031 grammar and is recorded by commit `54bffc6a1b5f77bc1f2f04d629142970ea7dcfa2`, although the immutable handover necessarily retains its contemporaneous `SELF` placeholder.

The workflow-only commits `c8593f294dc720e81fb1438aad98419a12b760ff` and `10a1fc4d6b064324bf4c6c6a3db102b55827c226` do not change the source evidence for F-031.

## Risks

The 2026-07-18 report describes an older baseline, so every finding must be revalidated against the current tree before implementation.

Cleanup or intervening fixes may resolve, narrow, invalidate, or create a prerequisite for a later finding.

The plan becomes misleading if evidence is checked before validation passes or if cross-finding effects are not recorded immediately.

## Plan

### Iteration protocol

Apply this protocol to exactly one finding at a time:

1. Revalidate the report evidence, current implementation, tests, documentation, completed-ticket overlap, dependencies, and effects from prior iterations.
2. Stop for user input when D-174 requires it: a product or architecture choice, dependency, binary-chain change, weaker security boundary, Opportunity, unresolved or rejected finding/item, materially expanded or adjacent scope, contradictory or stale evidence, unclassifiable failure, or anything that no longer makes sense. D-174's complete recorded stop list remains controlling.
3. Record exact scope, non-goals, security boundary, acceptance evidence, source/test allowlist, and validation commands in this plan, then obtain fresh independent scope review before cleanup or implementation.
4. Run finding-local and repository-wide cleanup discovery before the finding fix.
5. Accept only behavior-preserving, independently reversible simplifications with a concrete net reduction; review, validate, and commit each accepted cleanup separately.
6. Implement the smallest coherent correction with one implementation writer.
7. Give every cleanup and fix unit a fresh independent review, a separate fix-worker pass for accepted corrections, and repeated fresh review after corrections until the final diff receives a PASS; run focused and boundary-required validation plus controller scope/diff/security checks before its local commit.
8. Record ordered commits, review results, validation evidence, cross-finding effects, unavailable checks, and remaining risks in this plan.
9. Mark the finding complete only after its acceptance evidence passes, then promote the next finding.

Routine per-finding approval and completion prompts are not required under D-174.

After F-031 through F-037 are resolved or explicitly closed, the medium → low transition requires a focused `ask_user` decision with comment entry enabled before F-038 starts.

### Completed iteration — F-031 strict privileged configuration grammar

D-200 directly reproduced late failure for relative `backend.caFile`, relative `accessPaths`, and tab-bearing names.

D-201 approved these first-release rules:

- instance names match `[a-z][a-z0-9_-]{0,20}`;
- wrapper names match `[A-Za-z0-9][A-Za-z0-9._+-]{0,127}` and still reject `debug`;
- privileged paths are canonical absolute paths;
- `accessPaths` additionally permits exactly `~` and canonical `~/...`;
- whitespace, controls, empty components, `.` and `..`, traversal, duplicates, and overlaps are rejected where applicable;
- CA and client-identity paths are absolute;
- resource names remain unchanged.

Security boundary:

- Protected asset: integrity and availability of root-generated users, groups, wrappers, services, daemon configuration, target-UID authority rows, and privileged path resolution.
- Failure source: an operator configuration mistake or an invalid non-Nix daemon configuration; local callers do not control this boundary.
- Boundary direction: Nix options or daemon YAML → root-owned artifacts and privileged runtime consumers.
- Invariant: invalid names and paths fail at Nix evaluation and `DaemonConfig::validate()` before malformed trusted artifacts, service startup, or wrapper invocation.

Exact scope:

- Add shared lexical name/path predicates in `nix/modules/underwrap.nix` and mirror them in `crates/underwrap-protocol/src/config.rs`.
- Validate instance and wrapper names with D-201's exact token grammars.
- Require canonical absolute lexical paths for `socket`, `ns_binary`, `debug_tool_target`, `target_runtime_dir_base`, wrapper `target`, optional `backend.ca_file`, optional `backend.client_identity_file`, and `env_set.{HOME,XDG_CONFIG_HOME,XDG_DATA_HOME,XDG_CACHE_HOME,XDG_STATE_HOME}`.
- Apply the same shared canonical-absolute predicate to `underwrap-keygen` before either a config-derived or explicit client-identity output path reaches key generation or filesystem mutation.
- Keep `files.paths` canonical target-home-relative paths and reject duplicates or ancestor/descendant overlap in that relative domain.
- Admit each `access_paths` value only as a canonical absolute path, exact `~`, or canonical `~/...`; reject duplicates or ancestor/descendant overlap within the absolute domain and separately within the tilde domain.
- Do not claim that daemon startup can compare an absolute `access_paths` value with a tilde value before caller-home resolution.
- Keep lexical validation independent of filesystem existence so evaluation and startup remain deterministic.
- Reject applicable invalid names, paths, duplicates, and overlaps before target-UID table generation or idmap planning.
- Extend the existing Nix assertion matrix and Rust sibling tests with exact valid boundaries and invalid whitespace, control, length, component, traversal, duplicate, and overlap cases.
- Update current configuration/testing documentation only where the supported grammar or evidence changes.

Expected source/test allowlist:

- `nix/modules/underwrap.nix`
- `nix/tests/frontend-security-assertions-eval.nix`
- `crates/underwrap-protocol/src/config.rs`
- `crates/underwrap-protocol/src/config/tests.rs`
- `crates/underwrap-keygen/src/lib.rs`
- `crates/underwrap-keygen/src/tests.rs`
- `crates/integration-tests/src/tests/directory_and_status.rs`
- `docs/configuration.md`
- `docs/testing.md`
- this plan for evidence

Non-goals:

- no resource-name, backend entity-name, backend URL, arbitrary `envSet` value, or `envSet.PATH` restriction;
- no filesystem `realpath`, existence, ownership, or symlink-resolution requirement;
- no dependency, protocol, target-UID derivation, idmap algorithm, wrapper-chain, or runtime behavior change beyond earlier deterministic rejection;
- no F-032 or later finding, Opportunity, or unresolved item.

Validation commands:

- `cargo test -p underwrap-protocol config`
- `cargo test -p underwrap-keygen`
- `cargo test --workspace --exclude integration-tests`
- `./crates/integration-tests/run`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `nix build .#checks.x86_64-linux.frontend-security-assertions-eval`
- `nix build .#checks.x86_64-linux.examples-nixos-eval`
- `nix build .#checks.x86_64-linux.workspace-test`
- `nix flake check --all-systems --no-build`
- `nix build .#checks.x86_64-linux.vm-test`

- [x] Revalidate F-031 against the current tree: `git diff --name-only 54bffc6..10a1fc4` changes only workflow guidance and skills; direct inspection confirms `nix/modules/underwrap.nix` and `DaemonConfig::validate()` still accept the D-200 classes. Evidence: direct, 2026-08-26.
- [x] Obtain fresh independent review of the exact scope, non-goals, boundary, allowlist, and validation above before cleanup discovery. Evidence: direct read-only review PASS on 2026-08-27; no commands were run.
- [x] Complete finding-local and repository-wide cleanup discovery and record every accepted or rejected candidate with its reason.
  - Finding-local discovery was exhausted after rejecting marginal helper inlining, fix-coupled path/overlap changes, and premature test abstractions.
  - Repository-wide discovery accepted one test-only response-decoding consolidation.
  - Implementation commit `bdc8cc50eebfc41d1944a2122adbc7966dbf029a` (`test: simplify runtime response decoding`) replaces 13 redundant JSON round trips.
  - Independent review found a secret-bearing panic diagnostic.
  - A separate fix-worker removed the body copy/output, and fresh re-review passed.
  - Direct validation: web build passed; 30 focused backend runtime API tests passed; backend all-target Clippy with warnings denied, rustfmt, and diff checks passed.
  - Generated `web/dist` was removed.
  - Controller scope/security check: exact three test-only paths, no production behavior or secret output, no F-id resolved; F-054 remains separate.
- [x] Correct the F-031 scope after independent implementation review found that `underwrap-keygen` partially parses `BackendServiceConfig` without `DaemonConfig::validate()`. Evidence: direct source trace on 2026-08-27. This is a necessary direct client-identity-path consumer, not a product choice or material adjacent expansion. The allowlist adds only keygen's implementation and sibling tests, validation adds `cargo test -p underwrap-keygen`, and a fresh independent scope review is required before correction.
- [x] Obtain fresh independent approval of the keygen scope correction. Evidence: direct read-only PASS on 2026-08-27; both config-derived and explicit output modes belong to the same approved path grammar, and sharing the protocol predicate is the smallest correction.
- [x] Correct validation scope after the unsupported raw `cargo test --workspace` invocation exposed five stale integration assertions for the deliberately changed early validation messages. Evidence: direct run on 2026-08-27 produced 95 passes and 10 failures: four expected missing runner-provided binaries, five exact stale message checks in `directory_and_status.rs`, and one connection refusal after fixture failures. Replace the unsupported aggregate invocation with non-integration workspace tests plus the repository integration runner, and add only `directory_and_status.rs` to align those directly affected assertions. This is test evidence for F-031, not adjacent product scope.
- [x] Obtain fresh independent approval of the integration-test scope correction. Evidence: direct read-only PASS on 2026-08-27; exactly five assertions are stale, the runner is the supported complete integration command, and no harness or product expansion is justified.
- [x] Mirror D-201 name and path grammar in Nix evaluation and every runtime consumer without narrowing resource names.
  - Fix commit `117e51b7ded3bdbe38bdee3e1c543184c8ea1bd6` (`fix: reject invalid privileged configuration early`) enforces the grammar in Nix, `DaemonConfig`, and both keygen output modes.
  - Invalid instance and wrapper selectors cannot generate users, groups, shims, services, daemon files, or target-UID rows before evaluation fails.
  - Resource names, backend URL semantics, arbitrary `envSet`, and `envSet.PATH` remain unchanged.
- [x] Add positive boundary cases and negative name/path/duplicate/overlap cases that fail at Nix evaluation and runtime validation before malformed trusted artifacts or service startup.
  - Rust covers 20 configuration tests and 14 keygen tests.
  - The Nix matrix pins 18 assertion messages, 81 exact-singleton negative rows, and 7 positive rows, including Unicode parity, exact `~`, mixed access domains, and derivation-backed CA paths.
  - Five integration scenarios prove the earlier canonical-path errors through daemon startup.
- [x] Run focused protocol and Nix evaluator checks, `cargo fmt -- --check`, relevant Clippy/workspace checks, all-system Nix evaluation, and `nix build .#checks.x86_64-linux.vm-test`.
  - Direct PASS: protocol configuration tests, keygen tests, non-integration workspace tests, 105 self-contained integration-runner tests, workspace Clippy with warnings denied, rustfmt, focused assertion evaluator, examples evaluation, workspace Nix tests, all-system no-build evaluation, and the NixOS VM with `ALL TESTS PASSED`.
  - The first raw `cargo test --workspace` attempt was unsupported by F-027's runner contract and exposed five stale assertions; the supported split commands passed after their reviewed correction.
  - The first all-system no-build attempts reached an unrealized VM fingerprint derivation; building the VM realized the direct input, the VM passed, and the unchanged all-system evaluation then passed.
  - Generated `web/dist` was removed.
- [x] Complete independent review, separate correction passes, controller checks, coherent commits, and the F-031 evidence update in this plan.
  - Review corrections covered secret-free cleanup diagnostics, Unicode whitespace/control parity, deterministic wrapper error order, exact-tilde evidence, keygen's direct client-identity path consumer, directly affected integration assertions, Clippy, and coercible Nix derivation paths.
  - Every correction used a separate fix-worker and received fresh re-review; final cleanup and F-031 reviews passed.
  - Controller check: exact approved scope, no dependency or wrapper-chain change, no weakened boundary, no secret-bearing diagnostics, no generated artifact, and no unrelated diff.
  - F-031 is resolved; F-030 evidence remains intact, and F-032 through F-058 remain open.

### Next iteration — F-032 unsafe invariant rationale

F-032 has not yet been revalidated against the current tree.

Do not begin cleanup or implementation until its current evidence, exact scope, security boundary, allowlist, validation, and fresh independent scope review replace the queue row below.

### Remaining medium queue

Promote one row at a time by replacing it with exact iteration tasks after revalidation.

- [ ] F-032 — add explicit invariant rationale for security-critical unsafe blocks.
- [ ] F-033 — reduce privileged namespace dependency breadth.
- [ ] F-034 — document offline administrator lockout recovery.
- [ ] F-035 — correct resource-name versus resource-id vocabulary.
- [ ] F-036 — restore visible keyboard focus on dialog and drawer close controls.
- [ ] F-037 — reconcile stale store routes and list-error handling.
- [ ] Obtain and record the mandatory medium → low transition decision.

### Remaining low queue

After the transition gate, promote one row at a time using the same iteration protocol.

- [ ] F-038 — enforce the JCS safe-integer timestamp limit.
- [ ] F-039 — update `Collection.Modified` for item mutations.
- [ ] F-040 — correct the `underwrap-secretsd-store` persistence Rustdoc.
- [ ] F-041 — scope persistent-secret-store claims to brokered resources.
- [ ] F-042 — document the daemon's resource-blob parsing accurately.
- [ ] F-043 — remove no-op scope and duplicated helper fields.
- [ ] F-044 — preserve forced XDG environment precedence.
- [ ] F-045 — reject malformed or dangling Secret Service aliases.
- [ ] F-046 — reject wrong-typed `CreateItem` properties.
- [ ] F-047 — correct profile session-revocation guidance.
- [ ] F-048 — consume expected confirmation mutation rejections.
- [ ] F-049 — redact internal backend error chains from responses.
- [ ] F-050 — remove stale master-key configuration from Playwright fixtures.
- [ ] F-051 — remove stale workflow and temporal wording from published docs.
- [ ] F-052 — correct audit and writeback comments and vocabulary.
- [ ] F-053 — add real tablet Playwright coverage or remove the unsupported project claim.
- [ ] F-054 — strengthen encoded and full-blob log-hygiene evidence.
- [ ] F-055 — remove stale SSH setup and describe the loopback-only VM scenario accurately.
- [ ] F-056 — make strict Rustdoc pass and add the gate.
- [ ] F-057 — declare the Rust support contract and resolver choice.
- [ ] F-058 — refresh and record the supported frontend and OpenAPI tool matrix.

## Completion Checklist

- [ ] F-031 through F-037 are resolved or explicitly closed with current evidence.
- [ ] The medium → low transition is explicitly approved and recorded.
- [ ] F-038 through F-058 are resolved or explicitly closed with current evidence.
- [ ] Every accepted cleanup and fix has independent review, required validation, controller checks, and an ordered local commit recorded here.
- [ ] Every cross-finding effect, stop decision, unavailable check, and residual risk has a durable owner.
- [ ] Accepted decisions, current mechanisms, research, future direction, and user-visible behavior have moved to their maintained documentation owners.
- [ ] No active ticket or competing plan owns any remaining obligation.
- [ ] The user accepts final program completion before this plan moves to `docs/plans/archived/`.
