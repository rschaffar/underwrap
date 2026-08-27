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

## Current-tree revalidation survey

On 2026-08-27 the user directed the controller to adapt this plan to the current findings and continue through all of them. This confirms continued one-finding-at-a-time execution under D-174; it does not waive any stop condition or the mandatory medium → low transition gate.

Direct current-tree inspection at `0b7d971a0808aa516130377e476bc12f69b6cf45`, four independent domain passes, and a separate challenge review found no remaining finding that can be closed without work:

- F-033, F-034, F-036, F-037, F-039, F-041, F-042, F-044, F-046 through F-048, F-050, F-052, F-053, and F-055 through F-057 persist substantially as reported.
- F-032, F-035, F-038, F-043, F-049, and F-054 remain valid with narrower current evidence because intervening work addressed part of each finding.
- F-040, F-045, F-051, and F-058 require a current recommendation rather than mechanical execution of stale report wording.

The required later adaptations are:

- F-040 describes the in-memory store → private host-publication channel; it must not restore the superseded tmpfs state pathname.
- F-045 records that strict host validation and commit-after-ACK prevent malformed or dangling graphs from committing, while `SetAlias` still lacks early `InvalidArgs` validation and an unexpected non-root collection path can be interpreted as alias removal and persist that unintended valid mutation.
- F-051 retains valid plan/history navigation and removes only remaining stale temporal or current-state wording.
- F-058 separates current support-matrix documentation and compatible patch/minor refreshes from major frontend/tool upgrades; major upgrades require the applicable D-174 decision before implementation.

Direct executable probes confirmed that the tablet Playwright project still discovers zero tests and that strict denied-warning Rustdoc still fails with invalid HTML, a broken `argv[0]` link, private-module links, and no aggregate gate. Registry comparison also confirms that some F-058 packages are current while Vite, Vitest, and TypeScript remain behind current majors; latest-version numbers alone do not establish compatibility.

No low finding is promoted by this survey. F-038 remains blocked until F-032 through F-037 are complete or explicitly closed and the user approves the medium → low transition with comment entry enabled.

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

### Current iteration — F-032 unsafe invariant rationale

The report's lexical inventory of 148 production unsafe blocks and 20 `SAFETY` markers is obsolete after the privileged lifecycle work completed for F-006 through F-031.

A direct current scan excluding integration-test code, sibling test modules, and test support finds 220 `unsafe { ... }` expressions across 28 production files and 104 `SAFETY:` markers. The counts are only discovery evidence: one comment can cover multiple expressions, and only semantic per-site inspection can establish whether pointer, lifetime, initialization, descriptor-ownership, syscall, and privilege invariants are stated correctly.

Current source confirms that many newer lifecycle blocks carry useful rationale while older sites such as `underwrap-protocol/src/scm_rights.rs` still do not. The finding therefore remains valid with updated scope.

Security boundary:

- Protected assets: brokered resource confidentiality and integrity, authenticated fd 20/fd 3 ownership, target identity and privilege transitions, process lifecycle, runtime client identity, and backend state touched by syscall/FFI code.
- Attacker or failure source: the untrusted local caller, untrusted target protocol input, malformed descriptors or frames, and kernel/syscall failure interacting with an incorrectly maintained unsafe precondition.
- Boundary direction: validated Rust state and owned descriptors/buffers → libc/kernel FFI → privileged or secret-bearing process state.
- Preserved invariant: this iteration changes no executable behavior; every production unsafe expression has an adjacent rationale sufficient for a reviewer to verify pointer validity, initialization, lifetime, alignment where applicable, raw-fd ownership, syscall arguments, and return-value interpretation. Discovery of an actual defect stops this comment-only iteration for separate scope.

Exact scope:

- Semantically inspect every production `unsafe { ... }` expression under `crates/*/src`.
- Exclude `crates/integration-tests`, sibling `tests.rs`/`tests/` modules, and `test_support.rs`; test-only unsafe code is not evidence for the production FFI boundary in F-032.
- Preserve every already adequate rationale and add concise immediately adjacent `// SAFETY:` comments where the invariant is absent or incomplete.
- Keep comments specific to the operation; do not use generic statements such as “the call is safe.”
- Do not regroup calls, change scopes, refactor FFI, alter error handling, add a lint/gate, or modify executable tokens as part of the finding fix.
- If semantic review finds a real pointer, ownership, privilege, descriptor, syscall, or error-handling defect, stop and record the evidence rather than hiding a behavior change inside F-032.

Approved O-008 expansion after the mandated stop:

- The first semantic pass stopped without edits after finding that `scm_rights.rs` allocates ancillary storage as `Vec<u8>` and then dereferences it as `cmsghdr` and copies through `RawFd`; Rust guarantees only byte alignment for that allocation, so the unsafe precondition cannot be justified.
- Independent read-only confirmation classified this as undefined behavior under Rust's alignment guarantees.
- On 2026-08-27 the user explicitly selected **Implement full O-008 now**, approving a broader executable correction before the remaining comment-only pass.
- Replace ancillary storage with correctly sized and aligned `MaybeUninit<cmsghdr>` storage for send and receive.
- Make ownership explicit: `send_fd` accepts borrowed descriptors, `recv_fd` returns `OwnedFd`, and production/test callers use `AsFd` rather than constructing a temporary owning `UnixStream` over a borrowed raw descriptor.
- Create SCM_RIGHTS socketpairs with `SOCK_CLOEXEC` and receive with `MSG_CMSG_CLOEXEC` so both local and received descriptors acquire close-on-exec atomically.
- Require exactly one received SCM_RIGHTS descriptor. Adopt every kernel-installed descriptor before validation completes so truncation, extra descriptors, an unexpected control message, or any later error drops all received ownership without leaks.
- Thread the owned received descriptor through `underwrap-exec` relocation; do not convert it back to an ambiguously owned integer before the deliberate fixed-fd mapping.
- Add structural alignment, socketpair/received CLOEXEC, exact-one success, extra/truncated rejection-with-closure, unexpected-control-plus-SCM_RIGHTS rejection-with-closure, and existing round-trip evidence.
- Update the maintained security mechanism description without claiming formal proof, cross-platform support, or general descriptor noninterference.

O-008 security boundary:

- Protected asset: the scoped resource-channel capability and every process descriptor table that must not retain it across an unrelated exec or malformed receive.
- Attacker or failure source: a malformed ancillary message, future concurrent exec, allocator alignment permitted by Rust but insufficient for C structures, or an ownership error on a rejection path.
- Boundary direction: daemon-owned scoped channel → SCM_RIGHTS kernel transfer → setgid entry-stage owned descriptor → deliberate fd-20 handoff.
- Invariant: control storage is correctly aligned; each installed descriptor has exactly one Rust owner immediately; all local and received descriptors are CLOEXEC until deliberate `dup2` to fd 20; malformed or multi-descriptor input fails closed and drops every received descriptor.

O-008 source/test allowlist:

- `crates/underwrap-protocol/src/scm_rights.rs`
- `crates/underwrap-protocol/src/scm_rights/tests.rs`
- `crates/underwrapd/src/wrapper_flow/channels.rs`
- `crates/underwrap-exec/src/main.rs`
- `crates/underwrap-exec/src/main/tests.rs`
- `crates/integration-tests/src/tests/mod.rs`
- `crates/integration-tests/src/tests/wrapper_writeback.rs`
- `docs/security.md`
- this plan for evidence

O-008 non-goals:

- no new dependency, alternate transport, protocol message, fixed-fd number, daemon authorization, channel multiplicity, timeout, retry, wrapper target, or backend behavior;
- no underwrap-ns fd-20 authentication or lifecycle change and no general conversion of unrelated raw-fd code;
- no O-020 generated gate, F-033 dependency split, or other Opportunity/finding.

O-008 validation commands:

- `cargo test -p underwrap-protocol scm_rights`
- `cargo test -p underwrap-exec`
- `cargo test -p underwrapd`
- `cargo test --workspace --exclude integration-tests`
- `./crates/integration-tests/run`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `nix build .#checks.x86_64-linux.workspace-test`
- `nix flake check --all-systems --no-build`
- `nix build .#checks.x86_64-linux.vm-test`

Approved libsecret post-fork correction after the second mandated stop:

- The resumed semantic pass stopped because both libsecret helper child branches raw-close descriptors that remain owned by inherited `OwnedFd`/`UnixStream` values, then run panic-capable Rust before exec or `process::exit`.
- With unwind enabled, a panic can run stale owners after a descriptor number has been reused and close an unrelated helper, publication, readiness, logging, or lifecycle endpoint. Independent review confirmed the ownership defect; no direct secret disclosure was established.
- On 2026-08-27 the user selected **Fix narrowly in F-032**, approving an ownership-correct and unwind-contained correction without a dependency or binary-chain change.
- Consume every child-unneeded owned endpoint with `drop` rather than raw-closing its borrowed integer.
- In the dbus-daemon child, consume readiness reader/writer, address reader, init reader/writer, and publisher; retain the gate as CLOEXEC; clear CLOEXEC only on the address writer; and map the helper log to stderr.
- Before the secretsd fork, preserve the existing parent consumption of both address endpoints. In the secretsd child, consume the readiness reader and init writer; retain the gate as CLOEXEC; clear CLOEXEC only on init reader, readiness writer, and publisher; and map the helper log to stderr.
- Do not raw-close the borrowed exec-gate descriptor while its `GateWriter` owner remains live. Verify and preserve its kernel-set CLOEXEC state so successful helper exec closes it atomically.
- Contain every child action from the first endpoint consumption through setup, diagnostics, and returned-exec handling in one `catch_unwind(AssertUnwindSafe(...))` boundary. The assertion is justified because the single-threaded child never exposes or resumes captured state after either success or panic.
- After the boundary returns or catches, call `_exit` immediately without formatting, logging, allocation, or panic/error-payload destruction. Abort or double-panic may terminate without being caught; neither may unwind into inherited PID-2 state.
- Preserve helper executable selection, arguments, sanitized environment, logging paths, readiness/address/init/publication endpoint roles, allocation order, and parent behavior.
- Add fork-isolated controlled-error and injected-panic evidence for both endpoint matrices proving `_exit` status and absence of a return marker.
- Prove the gate remains open with `FD_CLOEXEC`, only selected endpoints have CLOEXEC cleared and remain usable before exec, and a descriptor number reused after consuming an unwanted owner remains live through injected unwind; an `EBADF`-only assertion is insufficient.
- Preserve deployed argv, environment, log, readiness, publication, store/lookup/writeback, live private-bus, host-only receiver, and secretsd-only publisher evidence.

Libsecret correction security boundary:

- Protected asset: integrity and exact lifetime of exec-gate, readiness, address, init, publication, log, and helper descriptors in the secret-bearing PID-2 setup branch.
- Attacker or failure source: an ordinary setup I/O error or panic after raw close but before helper exec, followed by descriptor-number reuse during unwinding.
- Boundary direction: PID-2 owned Rust descriptors → post-fork helper child setup → dbus-daemon or secretsd exec.
- Invariant: every owned descriptor closes exactly once; no panic unwinds into inherited PID-2 state; unneeded endpoints are consumed before helper-visible work; selected endpoints alone have CLOEXEC cleared; all failure paths `_exit` without exposing an alternate channel.

Libsecret correction allowlist:

- `crates/underwrap-ns/src/libsecret.rs`
- `crates/underwrap-ns/src/libsecret/tests.rs`
- `docs/security.md`
- `docs/testing.md`
- this plan for evidence

Libsecret correction non-goals:

- no helper, target, protocol, channel, fd number, namespace, mount, privilege, publication, readiness, environment, log-content, dependency, or wrapper-chain redesign;
- no general post-fork abstraction outside the two libsecret helper branches and no later finding or Opportunity.

Libsecret correction validation commands:

- `cargo test -p underwrap-ns libsecret`
- `cargo test -p underwrap-ns`
- `cargo test --workspace --exclude integration-tests` with the exact Nix `web-build` artifact
- `./crates/integration-tests/run`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `nix build .#checks.x86_64-linux.workspace-test`
- `nix build .#checks.x86_64-linux.vm-test`, including the existing libsecret store/lookup/writeback, live private-bus, host-only receiver, and secretsd-only publisher assertions
- `nix flake check --all-systems --no-build`

Expected source allowlist:

- `crates/underwrap-backend/src/{cli.rs,main.rs}`
- `crates/underwrap-debug-tool/src/main.rs`
- `crates/underwrap-exec/src/{main.rs,supervision.rs}`
- `crates/underwrap-keygen/src/lib.rs`
- `crates/underwrap-ns/src/{boundary.rs,caps.rs,directory_credential.rs,exec_gate.rs,handoff.rs,idmap.rs,libsecret.rs,main.rs,mounts.rs,publication.rs,resource_channel.rs,supervision.rs,target_status.rs,target_uid.rs,writeback.rs}`
- `crates/underwrap-protocol/src/scm_rights.rs`
- `crates/underwrap-secretsd/src/{main.rs,publication.rs}`
- `crates/underwrapd/src/{main.rs,peer_auth.rs,startup.rs,wrapper_flow/channels.rs}`
- this plan for evidence

Non-goals:

- no production behavior, FFI shape, dependency, protocol, wire format, error, log, target, privilege, descriptor, or wrapper-chain change;
- no test-only unsafe annotation, generated gate, lint configuration, formal proof, or claim that comments replace tests;
- no O-008 SCM_RIGHTS redesign, O-020 documentation gate, F-033 dependency work, or later finding.

Validation commands:

- `cargo test --workspace --exclude integration-tests`
- `./crates/integration-tests/run`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `nix build .#checks.x86_64-linux.workspace-test`
- `nix flake check --all-systems --no-build`
- `nix build .#checks.x86_64-linux.vm-test`
- direct controller/reviewer inspection that the diff adds only accurate non-doc comments and plan evidence

- [x] Revalidate F-032 and replace the report's stale lexical counts with current direct evidence. Evidence: direct scan and source inspection plus four independent domain reviews and one challenge review on 2026-08-27; all agree the finding persists with narrower current evidence.
- [x] Obtain fresh independent approval of the exact F-032 scope, boundary, allowlist, non-goals, and validation above. Evidence: read-only PASS on 2026-08-27 independently reproduced the exact per-crate totals, 220-expression/28-file/104-marker inventory and allowlist, confirmed comment-only sufficiency, and found no absorbed Opportunity, later finding, or bypassed D-174/gate condition; no tests or builds were run.
- [x] Complete finding-local and repository-wide cleanup discovery; record every accepted or rejected candidate and independently process each accepted cleanup before the comment-only fix.
  - Accepted one independently reversible four-file cleanup: commit `509b6aa8e8ce716c7093f57276dd607792b71c0d` (`refactor: centralize namespace pipe creation`) consolidates four duplicate `pipe2(O_CLOEXEC)` plus immediate `OwnedFd` adoption implementations into one crate-local helper while preserving allocation order, endpoint roles, exact diagnostics, reserved-fd collision checks, and lifecycle behavior.
  - Fresh pre-implementation and final diff reviews passed; no correction worker was needed. Direct validation passed 114 `underwrap-ns` tests, package Clippy with warnings denied, rustfmt, 105 integration-runner tests after one connection-refusal transient passed on full rerun, and the NixOS VM with `ALL TESTS PASSED`.
  - Controller check: exact four-file allowlist, 22 net lines removed, immediate errno capture, exact one-time ownership adoption, no dependency/protocol/wrapper-chain/security behavior change, no secret diagnostics, no generated artifact, and no O-008/O-020/F-033 scope.
  - Rejected SCM_RIGHTS safe-fd/CLOEXEC redesign as O-008; a generated unsafe gate as O-020; cross-binary pipe, signal/wait, NSS, mount, syscall, hardening, or CLOEXEC abstractions as marginal or boundary-coupling; first resource-channel relocation removal as behavior-changing; no-op scope/config cleanup as F-043; and comment edits as the F-032 fix rather than cleanup.
  - Discovery is exhausted. The cleanup reduces the current comment-fix inventory to 211 unsafe expressions across 27 production files and 95 `SAFETY:` markers; those lexical counts remain discovery evidence only.
- [x] Stop the initial semantic pass when it found the SCM_RIGHTS ancillary-buffer alignment defect; do not add misleading rationale. Evidence: implementation worker stopped with no file changes on 2026-08-27, and a fresh independent read-only review confirmed the undefined-behavior precondition.
- [x] Obtain the required user decision for the adjacent Opportunity and executable security correction. Evidence: on 2026-08-27 the user selected **Implement full O-008 now**.
- [x] Obtain fresh independent approval of the exact O-008 scope, boundary, allowlist, non-goals, and validation above before implementation. Evidence: review identified one missing mixed unexpected-control-plus-SCM_RIGHTS closure case; the plan added it, and fresh re-review passed the corrected alignment, ownership, atomic-CLOEXEC, exact-one, error-closure, caller, scope, and validation matrix on 2026-08-27; no commands were run.
- [x] Implement and independently review the complete O-008 correction, run its validation, commit it as a separate fix unit, and record any effect on the remaining unsafe inventory.
  - Fix commit `b39b3b2a39c7399762be210ba4affc54473af26b` (`fix: make SCM_RIGHTS ownership atomic`) replaces under-aligned control storage, applies `SOCK_CLOEXEC`/`MSG_CMSG_CLOEXEC`, makes send borrowing and receive ownership explicit, rejects non-exact ancillary input, drops every adopted descriptor on rejection, and carries `OwnedFd` through entry-stage relocation.
  - Structural tests prove control-storage sizing/alignment, local and received CLOEXEC, exact-one round-trip, exact-extra and truncated descriptor closure, and mixed credentials/rights rejection with descriptor closure. Maintained security documentation now states the exact Linux mechanism and boundary.
  - Two initial independent FFI/lifecycle reviews passed. A controller challenge identified that payload reads should be explicitly bounded before the later `MSG_CTRUNC` check; a separate fix-worker added initialized-control-range checks, focused tests passed, and two fresh independent reviews passed the corrected FFI and ownership implementation.
  - Direct validation passed 6 SCM_RIGHTS tests, 14 entry-stage tests, 56 daemon tests, all non-integration workspace tests, 105 integration-runner tests, workspace Clippy with warnings denied, rustfmt, Nix workspace tests, all-system evaluation, and the NixOS VM with `ALL TESTS PASSED`.
  - The first plain workspace test attempt lacked the intentionally absent `web/dist`; the unchanged suite passed with the exact Nix `web-build` artifact through `UNDERWRAP_WEB_DIST`. One integration run had a connection-refusal fixture transient and all 105 tests passed on the full rerun.
  - Controller check: exact eight-file allowlist and staged checksum `5335dcd0ae5203d483eee2b06c4982adb201e18c966cfd6e0b32fdf84be6d5ed`; no dependency, protocol message, fixed-fd, authorization, backend, target, or underwrap-ns change; no descriptor leak/double ownership, secret-bearing diagnostic, generated artifact, later finding, or unapproved Opportunity.
  - O-008 is resolved under the user's explicit approval. It resolves no F-id by itself and reduces the remaining F-032 inventory to 209 unsafe expressions across 26 production files with 107 `SAFETY:` markers; counts remain discovery evidence only.
- [x] Stop the resumed semantic pass when it found the libsecret post-fork raw-close/owned-destructor defect; do not retain the unjustifiable rationale. Evidence: implementation worker completed a comment-only draft but partitioned review rejected the helper-child ownership claim, and a fresh focused review confirmed the executable defect on 2026-08-27.
- [x] Obtain the required user decision for the second adjacent executable correction. Evidence: on 2026-08-27 the user selected **Fix narrowly in F-032**.
- [x] Obtain fresh independent approval of the exact libsecret correction scope, boundary, allowlist, non-goals, and validation above before implementation. Evidence: initial review required exact endpoint matrices, a fully enclosing unwind boundary with immediate `_exit`, gate/selected/reused-fd tests, maintained testing documentation, a tighter allowlist, and named VM witnesses; the plan incorporated every correction and fresh re-review passed on 2026-08-27; no commands were run.
- [ ] Implement and independently review the libsecret post-fork correction, run its validation, commit it as a separate fix unit, and update the unsafe inventory.
- [ ] Correct every rejected or overstated rationale from the partitioned review, resume the semantic per-site pass over every remaining in-scope unsafe expression, and add adequate adjacent rationale without further executable behavior changes.
- [ ] Complete fresh independent implementation review, separate correction passes where accepted, all validation, controller scope/security/diff checks, and ordered local commits.
- [ ] Record F-032 completion and every cross-finding effect before promoting F-033.

### Remaining medium queue

Promote one row at a time by replacing it with exact iteration tasks after revalidation.
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
