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

### Completed iteration — F-032 unsafe invariant rationale

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
- Add exec-isolated child-process probes that invoke one exact test helper in a fresh test process for dbus/secretsd controlled-error and injected-panic cases without calling `fork` from the multithreaded Rust test harness. The probe emits a direct-write entry marker, then wraps endpoint construction plus the production no-return runner in an outer sentinel `catch_unwind`; either unexpected return or escaped panic `_exit`s 200. The parent requires the entry marker and exact production status 1 or 101, so libtest's own panic exit cannot make the panic case vacuously pass.
- Prove the gate remains open with `FD_CLOEXEC`, only selected endpoints have CLOEXEC cleared and remain usable before exec, and—in its own fresh process—a descriptor number reused after consuming an unwanted owner remains live through injected unwind; an `EBADF`-only assertion is insufficient.
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

Approved keygen reentrant NSS correction after the third mandated stop:

- Final rationale review rejected `underwrap-keygen`'s claim that public safe `Cli::resolve_targets` and `run` can rely on single-threaded `getpwnam`/`getgrnam` static storage.
- Concurrent safe library callers or unrelated same-process NSS lookups can replace that storage between FFI return and field dereference. Independent review confirmed potential undefined behavior; the shipped command's current single-threaded call pattern is not a valid public-API safety precondition.
- On 2026-08-27 the user selected **Fix reentrantly in F-032**, approving caller-owned `getpwnam_r`/`getgrnam_r` state without a dependency or binary-chain change.
- Replace static NSS lookup state with caller-owned output structures and buffers on the supported NixOS/Linux `x86_64` and `aarch64` surface; make no broader portability claim.
- Use `_SC_GETPW_R_SIZE_MAX`/`_SC_GETGR_R_SIZE_MAX` only as a hint. Zero, negative, or unavailable hints select a 16-KiB fallback; an unrepresentable or greater-than-1-MiB hint is an operational error. Allocate through checked fallible reservation.
- Grow only on `ERANGE`, using checked doubling capped at exactly 1 MiB. `ERANGE` at that maximum, arithmetic failure, or allocation failure aborts resolution. `EINTR` and every other non-`ERANGE` code propagate without retry or growth.
- Treat return zero plus a null result as ordinary not-found so the existing workstation/no-local-daemon ownership fallback remains. A successful non-null result must equal the caller-provided output-structure pointer before that structure is considered initialized; never dereference a foreign result pointer.
- Propagate NSS errors, invalid input, impossible result pointers, and exhausted buffer growth before key generation or identity-file mutation instead of silently treating them as no account. `harden_process()` remains the intentional earlier process-only side effect.
- Copy only the initialized UID/GID field before caller-owned storage is dropped; do not expose an NSS pointer or borrowed field.
- Preserve output-path selection and owner precedence: resolved daemon account, then existing identity owner, then invoking identity. Preserve explicit-path behavior, generated identity content, atomic installation, mode, stdout timing, and no-secret diagnostics.
- Add deterministic injected unavailable, zero, distinct negative, unrepresentable, and oversized hints; exact growth and exact-maximum `ERANGE`; arithmetic/allocation; `EINTR`/other error; not-found; and foreign-result-pointer tests. Prove every operational failure precedes key generation and identity-file mutation. Add real root/not-found lookup evidence and concurrent public `resolve_targets` evidence without relying on a data race to fail.
- Document fallback-versus-abort ordering and ownership precedence in `docs/security.md`, and enumerate injected growth/error/result-pointer plus concurrent public-API evidence in `docs/testing.md`.

Keygen NSS security boundary:

- Protected asset: correct restrictive ownership and availability of the frontend runtime-client identity file.
- Attacker or failure source: concurrent same-process calls through the public safe keygen API or unrelated NSS activity replacing static libc storage.
- Boundary direction: public safe Rust configuration resolution → libc/NSS account lookup → selected UID/GID for atomic identity installation.
- Invariant: every FFI output and backing string buffer is caller-owned for the complete dereference lifetime; not-found alone selects the documented fallback, while operational lookup failure aborts before key generation or filesystem mutation.

Keygen NSS allowlist:

- `crates/underwrap-keygen/src/lib.rs`
- `crates/underwrap-keygen/src/tests.rs`
- `docs/security.md`
- `docs/testing.md`
- this plan for evidence

Keygen NSS non-goals:

- no account naming, configuration, output-path, ownership precedence, file mode, credential format, key generation, persistence, stdout, caching, dependency, daemon, backend, or wrapper-chain redesign;
- no general NSS refactor in private single-threaded privileged binaries and no later finding or Opportunity.

Keygen NSS validation commands:

- `cargo test -p underwrap-keygen`
- `cargo test --workspace --exclude integration-tests` with the exact Nix `web-build` artifact
- `./crates/integration-tests/run`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `nix build .#checks.x86_64-linux.workspace-test`
- `nix build .#checks.x86_64-linux.vm-test`, including deployed exact identity ownership/mode and successful wrapper use
- `nix flake check --all-systems --no-build`

Approved daemon reentrant NSS correction after the fourth mandated stop:

- Final rationale review rejected `underwrapd::startup::resolve_group_gid`'s claim that daemon-startup ordering makes `getgrnam` static storage race-free.
- `#[tokio::main]` creates the multithreaded runtime before `server::run` resolves the configured instance group, so unrelated same-process NSS activity can replace the storage before the safe wrapper copies `gr_gid`.
- Independent review confirmed potential undefined behavior and traced the sole production path as `server::run` → `resolve_instance_gid` → `resolve_group_gid`.
- On 2026-08-27 the user selected **Fix locally in F-032**, approving a private caller-owned `getgrnam_r` correction without shared protocol code, a dependency, or a wrapper-chain change.
- Replace `getgrnam` and its static result with caller-owned `libc::group`, result-pointer storage, and backing bytes that remain live through exact result-pointer validation and the `gr_gid` copy.
- Treat `_SC_GETGR_R_SIZE_MAX` only as a hint, use a 16-KiB fallback for unavailable, zero, or distinct negative values, reject unrepresentable or greater-than-1-MiB values, and allocate with checked fallible reservation.
- Keep every requested buffer length and fallible reservation at or below exactly 1 MiB.
- Grow only on `ERANGE` by checked doubling capped at exactly 1 MiB, and propagate `ERANGE` at the maximum, arithmetic or allocation failure, `EINTR`, and every other operational error without binding the wrapper socket.
- Preserve an embedded-NUL error and exact ordinary not-found semantics, require return code zero plus a non-null result exactly equal to the caller-provided output pointer, and perform no initialized `libc::group` read before both conditions hold.
- Copy only initialized `gr_gid` before caller-owned storage is dropped.
- Make `resolve_group_gid` private because its sole production consumer is `resolve_instance_gid`, while retaining sibling-test access.
- Add deterministic injected evidence for unavailable, zero, distinct-negative, unrepresentable, and oversized hints; exact doubling; cap-to-1-MiB; success at the maximum; and `ERANGE` at the maximum with no further lookup.
- Add deterministic checked-arithmetic, distinct initial/growth reservation-failure, `EINTR`/other non-`ERANGE` without retry or growth, embedded-NUL-before-allocation, null-result, exact-pointer success with copied `gr_gid`, and foreign-pointer-without-dereference evidence.
- Add real known and generated-missing group lookups.
- Exercise `server::run` with otherwise valid configuration, a unique socket path, and a generated-missing group; require the lookup error and prove the wrapper socket was never created.
- Document the reentrant instance-group resolution mechanism and test evidence without claiming that unit tests prove NSS-provider or scheduler behavior.

Daemon NSS security boundary:

- Protected asset: the configured instance-group identity that gates the `0660` wrapper socket and therefore entry into the daemon's wrapper authorization flow.
- Attacker or failure source: unrelated same-process NSS activity or an NSS provider response interacting with shared static storage, oversized records, malformed result pointers, or operational failure.
- Boundary direction: root-owned daemon configuration → libc/NSS group lookup → copied expected GID → socket ownership verification and peer effective-GID authorization.
- Invariant: every FFI output and backing buffer is caller-owned for the complete dereference lifetime, only exact initialized success yields a GID, and every lookup failure aborts before socket binding without a weaker authorization fallback.

Daemon NSS allowlist:

- `crates/underwrapd/src/startup.rs`
- `crates/underwrapd/src/startup/tests.rs`
- `docs/security.md`
- `docs/testing.md`
- this plan for evidence

Daemon NSS non-goals:

- no configured group name, daemon account, socket path/mode/owner, effective-GID authorization, peer-auth posture, startup registration, retry, fallback identity, dependency, shared NSS abstraction, protocol, target, backend, or wrapper-chain redesign;
- no change to keygen, private single-threaded privileged-binary NSS lookups, later findings, or unrelated startup documentation claims.

Daemon NSS validation commands:

- `cargo test -p underwrapd startup`
- `cargo test -p underwrapd`
- `cargo test --workspace --exclude integration-tests` with the exact Nix `web-build` artifact
- `./crates/integration-tests/run`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `nix build .#checks.x86_64-linux.workspace-test`
- `nix flake check --all-systems --no-build`
- `nix build .#checks.x86_64-linux.vm-test`, including wrapper-socket GID, effective-GID peer acceptance/rejection, and successful deployed wrapper use

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
- [x] Correct the libsecret test scope after independent implementation review found that direct `fork` followed by allocation, panic handling, formatting, and fd work is not deterministic under Cargo's multithreaded test harness. Evidence: review on 2026-08-27; replace those cases with fresh exec-isolated exact-test subprocess probes and keep endpoint-state assertions process-local.
- [x] Obtain fresh independent approval of the corrected exec-isolated test design, including the outer sentinel boundary and direct probe-entry witness, before the implementation correction. Evidence: initial review caught the vacuous possibility that escaped panic and libtest could also exit 101; the outer sentinel and entry witness were added, and fresh re-review passed the exact process/status design on 2026-08-27; no commands were run.
- [x] Implement and independently review the libsecret post-fork correction, run its validation, commit it as a separate fix unit, and update the unsafe inventory.
  - Fix commit `316ef4cb83a7d68ab49cf028c1d0ec80db4561e1` (`fix: contain libsecret helper child ownership`) consumes unwanted endpoints through their Rust owners, verifies the borrowed gate remains CLOEXEC, clears CLOEXEC only on selected helper endpoints, and contains setup/diagnostics/returned-exec panic or error outcomes behind immediate `_exit`.
  - Endpoint matrices cover every dbus-daemon and secretsd endpoint. Exec-isolated probes use an exact helper filter, direct entry marker, outer sentinel unwind boundary, exact statuses, and a process-isolated exact-number reuse witness; no test calls `fork` from libtest.
  - Initial implementation review passed source ownership but rejected direct-fork tests as nondeterministic. A separate fix-worker replaced them; scope review found and corrected the vacuous libtest-101 possibility; fresh final review passed the source, probes, maintained security/testing docs, and exact four-file scope.
  - Direct validation passed 15 focused libsecret tests, 122 `underwrap-ns` tests, all non-integration workspace tests, 105 integration-runner tests, workspace Clippy with warnings denied, rustfmt, Nix workspace tests, all-system evaluation, and the NixOS VM with `ALL TESTS PASSED`.
  - The first VM run failed an F-007 unchanged-state disk equality after the invocation itself emitted `UsageSignal`, so the changed path had submitted no mutation; the unchanged derivation passed completely on rerun. This is classified as a VM lifecycle-fixture transient, not product evidence.
  - Controller check: exact four-file allowlist and staged checksum `465237b644aac1a2f2130c4a5c21908f78f2e9cb1245212966602be1baefc6e4`; no dependency, binary-chain, helper argument/environment/log, protocol, fd number, namespace, mount, privilege, publication, readiness, target, or later-finding change; no secret-bearing diagnostic or generated artifact.
  - The post-fork ownership defect is resolved and affects no other F-id. The current production inventory is 211 unsafe expressions across 26 files; the in-progress comment draft has 208 `SAFETY:` markers, and semantic adjacency still requires final correction and review.
- [x] Stop the final rationale pass when review found the public-safe keygen API's non-reentrant NSS lookup defect; do not retain the unenforceable single-threading rationale. Evidence: partitioned review and focused independent confirmation on 2026-08-27.
- [x] Obtain the required user decision for the third adjacent executable correction. Evidence: on 2026-08-27 the user selected **Fix reentrantly in F-032**.
- [x] Obtain fresh independent approval of the exact keygen NSS scope, boundary, allowlist, non-goals, and validation above before implementation. Evidence: initial review required exact bounded hint/growth/allocation, `EINTR`, result-pointer, platform, documentation, and test semantics; re-review then required distinct negative/unrepresentable hint evidence and pre-side-effect assertions. The plan incorporated every correction and fresh final re-review passed on 2026-08-27; no commands were run.
- [x] Implement and independently review the keygen NSS correction, run its validation, commit it as a separate fix unit, and update the unsafe inventory.
  - Fix commit `1691aa5fd6faf20c713897da65237bf59ed98da7` (`fix: make keygen NSS lookup reentrant`) replaces static NSS state with exact-result-checked caller-owned `getpwnam_r`/`getgrnam_r` output and bounded fallible buffers. Not-found alone preserves fallback; every operational failure aborts before key generation or identity mutation.
  - Tests cover every approved hint, growth, maximum, arithmetic, allocation, `EINTR`/other error, invalid input, not-found/success, foreign pointer, owner-precedence, real lookup, pre-side-effect, and concurrent-start smoke row.
  - Fresh FFI review passed. Evidence review rejected the initial wording as deterministic race proof; a separate fix-worker renamed and documented it as concurrent public-API smoke/stability evidence that does not prove overlap or race absence, and two fresh final reviews passed.
  - Direct validation passed 27 keygen tests, all non-integration workspace tests, 105 integration-runner tests, workspace Clippy with warnings denied, rustfmt, Nix workspace tests, all-system evaluation, and the NixOS VM with `ALL TESTS PASSED`.
  - One integration run saw two fixture-state authorization/reference failures; the unchanged full runner passed all 105 tests on rerun. This is classified as the existing real-process fixture isolation transient, unrelated to keygen NSS resolution.
  - Controller check: exact four-file allowlist and staged checksum `1d9fdb704ad3839769bf0b31f007b54cd4e1dc233d8410068b3a705898bbe4b6`; no dependency, CLI shape, output path, owner precedence, credential format/content/mode, generation, persistence, stdout, daemon/backend, protocol, wrapper-chain, later-finding, or secret-diagnostic change.
  - The public-safe NSS defect is resolved and affects no other F-id. The current production inventory remains 211 unsafe expressions across 26 files; the in-progress comment draft has 205 `SAFETY:` markers, and final semantic correction/review remains open.
- [x] Stop the next final rationale review when it found the daemon's non-reentrant group lookup defect; do not retain the false startup-order rationale. Evidence: partitioned review and focused independent call-path confirmation on 2026-08-27.
- [x] Obtain the required user decision for the fourth adjacent executable correction. Evidence: on 2026-08-27 the user selected **Fix locally in F-032** and then explicitly resumed the work.
- [x] Obtain fresh independent approval of the exact daemon NSS scope, boundary, allowlist, non-goals, and validation above before implementation. Evidence: initial review required an exact hint/growth/maximum/allocation/error/pointer matrix, a strict 1-MiB request bound, no initialized read before exact success, and a real `server::run` no-socket witness; the plan incorporated every correction and fresh re-review passed on 2026-08-27; no commands were run.
- [x] Implement and independently review the daemon NSS correction, run its validation, commit it as a separate fix unit, and update the unsafe inventory.
  - Fix commit `ce4c906178d4e362a52fb66fe4367e5b3658dcb5` (`fix: make daemon group lookup reentrant`) replaces static `getgrnam` storage with private caller-owned `getgrnam_r` output and bounded fallible buffers, validates exact success before copying `gr_gid`, and aborts every lookup failure before wrapper-socket binding.
  - Tests cover every approved hint, request bound, growth, exact maximum, arithmetic, reservation, non-`ERANGE`, invalid-input, not-found, exact/foreign-pointer, real NSS, and production `server::run` no-socket row.
  - Fresh FFI and evidence/scope implementation reviews passed without a correction worker.
  - Direct validation passed 25 focused startup tests, 68 daemon-library tests plus 2 binary tests, all non-integration workspace tests with the supported four-thread bound, 105 integration-runner tests, workspace Clippy with warnings denied, rustfmt, Nix workspace tests, and all-system evaluation.
  - The first raw workspace run saw one unrelated fd-number reuse assertion in the libsecret test module; the exact test and the unchanged supported four-thread workspace run passed.
  - Five exact-tree VM runs reached and passed daemon GID 992 resolution, `0660` socket publication, effective-GID peer acceptance/rejection, and deployed wrapper use, then stopped at varying F-007/F-009 large raw-blob equality assertions after an expected usage signal or no terminal mutation.
  - A clean detached worktree at unchanged HEAD `83b98152ec340914152fca979214e3ef44fb9a14` reproduced the same F-007 raw-blob equality failure without the daemon NSS implementation.
  - The full VM tail is therefore unavailable as non-regression evidence rather than a product regression; on 2026-08-27 the user explicitly accepted the bounded exact-tree VM evidence and required the harness failure to remain a separate durable obligation.
  - Controller check: exact four-file allowlist and staged checksum `cb6920ef425b67ae290db94b4854a14662b026f33e282424723c5a9526da0740`; no dependency, shared abstraction, configured group, daemon account, socket path/mode/owner, peer-auth, startup registration, retry, fallback identity, protocol, target, backend, wrapper-chain, later-finding, or secret-diagnostic change.
  - The daemon static-NSS defect is resolved and affects no other F-id. The current production inventory is 212 unsafe expressions across 26 files with 212 `SAFETY:` markers; those lexical counts remain discovery evidence only.
- [x] Correct every rejected or overstated rationale from the partitioned review, resume the semantic per-site pass over every remaining in-scope unsafe expression, and add adequate adjacent rationale without further executable behavior changes.
  - Comment commit `98245426056065d5db3da66b3fe3e41414973c2c` (`docs: explain unsafe invariants`) records operation-specific rationale for every current production unsafe expression.
  - Four fresh semantic partitions reviewed all 212 expressions across 26 files: backend/debug/keygen/daemon, both entry-stage files, target-identity/idmap paths, and the remaining namespace/protocol/Secret Service lifecycle paths all passed.
  - Accepted corrections remove the false post-fork syscall-only and generic async-signal-safety claims, prove `dup2` source/destination separation, describe fsid rollback as unverified best effort, and state exact NSS static-storage, fd-ownership, pointer, initialization, alignment, lifetime, syscall, return, and privilege preconditions.
  - A separate correction worker confirmed every accepted wording correction, and the four fresh final reviews required no further correction.
  - Existing non-SAFETY startup/test prose observations remain outside this comment-only finding and resolve no later F-id.
- [x] Complete fresh independent implementation review, separate correction passes where accepted, all validation, controller scope/security/diff checks, and ordered local commits.
  - Final-tree validation passed the supported bounded non-integration workspace suite, 105 integration-runner tests, workspace Clippy with warnings denied, rustfmt, Nix workspace tests, and all-system evaluation.
  - The user-accepted bounded VM evidence and separately owned unchanged-HEAD raw large-blob equality failure remain recorded above without a complete-VM-pass claim.
  - Controller inspection proves the final staged diff changes only non-doc comments in the 21-file approved subset, with no executable or doc-comment line and checksum `f9d4abf194ea5c3be1da3b8213e3b89aa9b3182be1d08ba9239198865b2db31f`.
  - A fresh independent source/security acceptance review passed the complete `cd899e0..9824542` range at a clean tree, confirming every actual unsafe-precondition defect is corrected and no dependency, protocol, binary-chain, security-boundary, later-finding, generated-artifact, secret-diagnostic, or unrelated executable change remains.
  - Initial evidence review correctly withheld closure until this plan recorded semantic disposition, exact final review, comments-only controller evidence, validation provenance, and cross-effects; fresh re-review passed the completed record on 2026-08-27.
- [x] Record F-032 completion and every cross-finding effect before promoting F-033.
  - Ordered functional units are cleanup `509b6aa`, SCM_RIGHTS `b39b3b2`, libsecret ownership `316ef4c`, keygen NSS `1691aa5`, daemon NSS `ce4c906`, and final rationale `9824542`; each scope and evidence commit appears in sequence above.
  - F-032 is resolved with semantic rationale at all 212 current production unsafe expressions; lexical parity is discovery evidence and does not claim formal proof or replace executable tests.
  - Explicitly approved O-008 is resolved; no remaining F-id is resolved, narrowed, or expanded by F-032, and F-033 remains the next medium finding.
  - The accepted VM raw large-blob equality-harness obligation remains separately owned by the unchecked task below and does not alter product behavior or F-032 closure.
- [ ] Resolve or explicitly close the large raw-blob VM equality-harness obligation before final program completion; it is non-blocking for F-032 under the user's bounded-evidence decision and has no implementation authority yet.

### Completed iteration — F-033 privileged namespace dependency profile

The report's dependency-direction finding remains directly reproducible at current HEAD `1818c387817b970934a0486f1a253f04b79bb0a8`.

An isolated locked `cargo check -p underwrap-ns` compiles protocol `config`, `control`, `entities`, `policy`, `service`, and `scm_rights` modules even though the namespace wrapper does not use them.

The same isolated `cargo tree -p underwrap-ns -e normal --depth 2` reaches protocol-owned Tokio, Utoipa, Serde YAML, and libc in addition to the canonical serde/JCS/blob/local-ABI dependencies and `underwrap-ns`'s own direct libc.

The fresh-target artifacts captured from clean HEAD have dep-info hash `83604055c6799c602bd495fe5662b43882986c5b008746593473eec12bb94dbf`, package-tree hash `5d73ca97a96cc4459a89a1bf62cba5549aac87ce21d7020d211b9b48a6c5371e`, and successful check-log hash `26b86e3d4fc0ac7987a53f1bae6bcfff71de4ce77ecf858360ac6a47d5c02da1`; the controller recalculated all three after drafting this scope.

This establishes compile and review coupling, not linked-code inclusion, runtime-TCB expansion, exploitability, or a dependency vulnerability.

Security boundary:

- Protected asset: the reviewable and rebuild-isolated source/dependency surface of the world-executable privileged namespace wrapper.
- Failure source: unrelated configuration, backend policy/admin, OpenAPI/schema, async transport, or SCM_RIGHTS maintenance entering the privileged crate's compile graph.
- Boundary direction: `underwrap-ns` manifest feature selection → compiled `underwrap-protocol` modules and normal dependencies → privileged namespace-wrapper package.
- Preserved invariant: the namespace wrapper consumes only canonical resource-domain and local ABI/codec code, while every wire value, serialization shape, fd number, privilege transition, namespace operation, target behavior, and default full protocol consumer remains unchanged.

Exact scope:

- Add two protocol profiles: default `full` and explicit `local-abi`; the local profile consists of always-compiled canonical/local modules, while `full` additionally enables configuration, control/entities/policy/service, async IPC, SCM_RIGHTS, and schema code.
- Make protocol-owned Tokio, Utoipa, and libc optional and enabled only by `full`.
- Move Serde YAML from protocol normal dependencies to dev dependencies because only protocol configuration tests use it.
- Move `AccessMode` into one always-compiled `access` module, update local internal/test imports, and re-export it from the full-only `config` module so existing full consumers retain the same path and type.
- Gate `config`, `control`, `entities`, `policy`, `service`, and `scm_rights` modules behind `full`.
- Gate async IPC functions/tests and Utoipa derives/manual schema implementations in always-compiled modules behind `full`.
- Select `underwrap-protocol` from `underwrap-ns` by direct workspace-relative path with default features disabled and only `local-abi` enabled; leave every other protocol consumer on the unchanged default full profile.
- Keep canonical serde, JSON/JCS, `anyhow`, base64, and zeroization dependencies in the local profile because the namespace wrapper directly consumes their local ABI and canonical resource-blob behavior.
- Document the isolated package feature boundary and exact dependency/source-module checks without claiming workspace-wide Cargo feature isolation, because a combined workspace graph may unify features from full consumers.

Expected source/test allowlist:

- `crates/underwrap-protocol/Cargo.toml`
- `crates/underwrap-protocol/src/lib.rs`
- new `crates/underwrap-protocol/src/access.rs`
- `crates/underwrap-protocol/src/config.rs`
- `crates/underwrap-protocol/src/exec.rs`
- `crates/underwrap-protocol/src/ipc.rs`
- `crates/underwrap-protocol/src/ipc/tests.rs`
- `crates/underwrap-protocol/src/message/common.rs`
- `crates/underwrap-protocol/src/message/wrapper.rs`
- `crates/underwrap-protocol/src/message/tests.rs`
- `crates/underwrap-protocol/src/resource.rs`
- `crates/underwrap-protocol/src/resource_blob.rs`
- `crates/underwrap-protocol/src/variant.rs`
- `crates/underwrap-ns/Cargo.toml`
- `crates/underwrap-ns/src/env/tests.rs`
- `crates/underwrap-ns/src/main/tests.rs`
- `docs/architecture.md`
- `docs/testing.md`
- this plan for evidence

Non-goals:

- no crate split, new dependency, dependency-version update, workspace-wide protocol-feature narrowing, default full-profile change for any non-namespace consumer, or Cargo resolver/support-contract change;
- no wire, serde/JCS, schema, OpenAPI, configuration grammar, resource model, fd, IPC framing, namespace, privilege, target, helper, writeback, backend, daemon, binary-chain, NixOS module, or deployed behavior change;
- no removal of dead ABI/config fields under F-043, terminology work under F-035, Rust support work under F-057, later finding, Opportunity, or unresolved item;
- no claim that excluding dead compile dependencies reduces the runtime TCB, linked binary, attack surface, or known vulnerability exposure;
- no weakening of the separately owned VM raw large-blob equality assertions merely to obtain a green validation run.

Validation commands and evidence:

- fresh-target `cargo check --locked -p underwrap-ns`, then inspect protocol dep-info and require absence of `config`, `control`, `entities`, `policy`, `service`, and `scm_rights` source paths
- `cargo tree --locked -p underwrap-ns -e features --depth 2`, requiring `local-abi` without protocol `default` or `full`
- `cargo tree --locked -p underwrap-ns -e normal,build --depth 2`, requiring no Tokio, Utoipa, or Serde YAML and no libc edge from `underwrap-protocol`; `underwrap-ns` retains its direct libc
- `cargo check --locked -p underwrap-protocol --no-default-features --features local-abi`
- `cargo test --locked -p underwrap-protocol --no-default-features --features local-abi`
- `cargo clippy --locked -p underwrap-protocol --no-default-features --features local-abi -- -D warnings`
- `cargo test -p underwrap-protocol`
- `cargo test -p underwrap-ns`
- `cargo test -p underwrap-exec`
- `cargo test -p underwrapd`
- `cargo test --workspace --exclude integration-tests -- --test-threads=4` with the exact Nix `web-build` artifact
- `./crates/integration-tests/run`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings` with the exact Nix `web-build` artifact
- `nix build .#packages.x86_64-linux.underwrap-ns`
- `nix build .#checks.x86_64-linux.workspace-test`
- `nix flake check --all-systems --no-build`
- `nix build .#checks.x86_64-linux.vm-test`; this F-033 boundary requires a complete pass, and reproduction of the separately owned raw large-blob equality failure leaves VM non-regression evidence unavailable and requires a fresh explicit decision rather than attribution as a pass

- [x] Revalidate F-033 against the current isolated package graph and replace the report's stale versions with exact current source/dependency evidence. Evidence: direct locked fresh-target check, dep-info inspection, Cargo tree, manifest/source trace, historical report/handover constraints, and two focused independent design/evidence consultations on 2026-08-27.
- [x] Obtain fresh independent approval of the exact F-033 profile design, security boundary, allowlist, non-goals, isolation evidence, and validation before cleanup discovery. Evidence: architecture review passed; evidence review required fresh-target/clean-tree precision, local-profile tests and Clippy, and a mandatory complete VM or fresh decision; the plan incorporated every correction, the controller recalculated all hashes, and fresh final re-review passed on 2026-08-27; no review commands were run.
- [x] Complete finding-local and repository-wide cleanup discovery; independently process each accepted cleanup before the feature-boundary fix.
  - Finding-local discovery proposed deleting the inferred three-line `[lib]` target declaration from `underwrap-protocol`; independent review rejected it as below-value, contrary to repository manifest consistency, and imminent same-file churn.
  - Repository-wide bounded definition/reachability discovery found no high-confidence independently useful deletion outside the fix paths.
  - Feature declarations, optional-dependency changes, Serde YAML reclassification, `AccessMode` extraction, cfg gates, import/test updates, dependency evidence, and docs are the F-033 fix rather than cleanup.
  - Dead ABI/config fields remain F-043; crate splits, broad module moves, test deletion, Nix changes, dependencies, generated surfaces, later findings, renames, formatting, and cosmetic churn are rejected.
  - Cleanup is exhausted with no accepted unit and no cleanup commit; the approved feature-boundary fix may begin.
- [x] Implement the approved full/local-ABI split with one writer and prove the isolated namespace package excludes every forbidden module and dependency while preserving default-full behavior.
  - Fix commit `1e99300b099f5897e8d5a0b49e89c558d7ccf417` (`refactor: isolate namespace protocol profile`) adds default `full` and explicit `local-abi` profiles, moves the identical `AccessMode` type to the always-compiled domain with a full-profile config re-export, and selects only local ABI from `underwrap-ns`.
  - Fresh-target dep-info excludes `config`, `control`, `entities`, `policy`, `service`, and `scm_rights`; its hash is `6187a08c6b68a21e31f5da2a05abc78496d3d1d63ee84190fa319c6531c724c5`.
  - The feature tree shows protocol `local-abi` without `default`/`full`, and the normal tree contains only `anyhow`, base64, serde, JSON/JCS, and zeroize under protocol while preserving `underwrap-ns`'s direct libc.
  - Final isolation evidence hashes are log `e5738c2c1a940d3c9e7f03e7cd4ab1fbd893457fa8925a48d1869e3e85ae289c`, feature tree `7f6d0c0a886cf328f1e58ed477da7dc65c8a9a98fb6322095bd6a72c6202a0fa`, package tree `496ef07a34e9879be4b5d0c218ce3d6e7a538657d202afff5bd786dad057e519`, and reverse-libc tree `1c0ca7d583f30b91c629506af8ee0095ea82f3bf695de46a21322a9cfe2c2d93`.
  - Every other direct protocol consumer retains the unchanged default full profile, and default protocol tests retain config, service, async IPC, SCM_RIGHTS, and schema evidence.
- [x] Complete fresh independent implementation review, separate correction passes where accepted, validation, controller scope/security/diff checks, coherent local commits, and plan evidence.
  - Fresh source/manifest and evidence/docs reviews passed the complete working diff without a correction worker.
  - A final constrained staged consult could not inspect Git's cached index and correctly withheld a result; a fresh clean-tree reviewer then inspected the exact commit and parent, independently matched all 18 paths and checksum, traced every consumer, and returned PASS.
  - Direct validation passed 59 local-profile and 115 default-full protocol tests, 122 namespace tests, 14 entry-stage tests, 68 daemon-library plus 2 daemon-binary tests, all bounded non-integration workspace tests, 105 integration-runner tests, local/full workspace Clippy with warnings denied, and rustfmt.
  - The first unbounded namespace runs saw the existing descriptor-number reuse assertion in one and then two helper-endpoint tests; each exact test and complete four-thread suite passed.
  - The first integration run saw one real-HTTPS TLS close-notify transient in the admin-reference matrix; its exact test and the unchanged full 105-test runner passed.
  - Nix validation passed the isolated `underwrap-ns` package, workspace tests, all-system evaluation, and the complete NixOS VM with `ALL TESTS PASSED`.
  - Controller check: exact 18-file allowlist and staged checksum `e8449088606d523102fef3a94af96696067146a72e070551b5a6436ab3dff5cb`; no Cargo lock/root manifest, new dependency, dependency version, wire/serde/schema/OpenAPI/config, fd, namespace, privilege, target, backend, daemon, Nix, binary-chain, generated, secret-diagnostic, later-finding, or unrelated change.
- [x] Record F-033 completion and every cross-finding effect before promoting F-034.
  - F-033 is resolved for isolated namespace-package compile/review coupling; combined workspace feature unification remains explicitly outside the claim.
  - No runtime-TCB, linked-binary, attack-surface, exploitability, or vulnerability reduction is claimed.
  - F-035, F-043, F-057, every other remaining finding, and the separately owned VM equality-harness obligation remain unresolved and unchanged.
  - Fresh final evidence/process re-review passed this completed record on 2026-08-27.
  - F-034 is the next medium finding.

### Completed iteration — F-034 offline operator recovery documentation

F-034 remains valid at current HEAD `66de836ca3a41a7b551f56ebf2b4ad762b093185` with narrower implementation evidence than the historical report.

`underwrap-backend --config <path> admin recover-user --username <name>` already reads one password line from stdin, normalizes the username, refuses an existing username, hashes the password, and atomically writes one new per-user record under the configured `users_dir` without receiving or changing the bootstrap sentinel.

The CLI help incorrectly says it appends directly to one user-state file, while `docs/admin-ui.md` and `docs/remote-backend.md` still omit the recovery flow entirely.

Security boundary:

- Protected assets: confidentiality of the recovery password, integrity of the operator registry, and every backend asset reachable through recovered admin authority.
- Failure source: a trusted backend-host operator responding to total login loss with manual hash/state edits, bootstrap-sentinel deletion, concurrent recovery, wrong file ownership, or password exposure through argv/environment/history.
- Boundary direction: trusted host-only stdin plus the backend service identity → config-selected service-owned user registry → one new operator record → ordinary HTTPS login and authenticated admin repair.
- Preserved invariant: no network or browser recovery route exists; the backend and every other user-registry writer stay stopped, the existing collection is loaded without overwrite, an existing normalized username is refused, and the bootstrap sentinel remains unchanged. The password enters the command through stdin, exists transiently in process memory, is never placed in argv/environment/logs, and only its Argon2id hash persists.

Exact scope:

- Make `docs/backend.md` the normative offline-recovery owner with the complete stopped-service, service-identity, stdin, non-overwrite, unchanged-sentinel, restart/login/repair, and temporary-user cleanup sequence.
- Add concise links and deployment-specific context from `docs/admin-ui.md` and `docs/remote-backend.md` rather than duplicating a competing procedure.
- Correct only the `recover-user` CLI help wording in `crates/underwrap-backend/src/main.rs` from a single appended state file to a new non-overwriting per-user record in the configured user registry.
- Preserve the existing command, username normalization, password acceptance, Argon2id hashing, random user id, atomic save, configured paths, log fields, bootstrap behavior, and error semantics.
- State that the command must run as the same backend service identity while the service/listener and every other user-registry writer are stopped.
- State that operators pass the password as one stdin line from a trusted prompt or password-manager pipe and never place it in argv, environment variables, configuration, or shell history.
- State that recovery adds a distinct user and cannot overwrite or reset an existing username; after ordinary login, use the authenticated Users flow to repair the intended account before optionally deleting the recovery user while another working operator remains.
- Warn against deleting the bootstrap sentinel, deleting the user registry, or manually editing Argon2 hashes as a recovery shortcut.

Expected source/documentation allowlist (exactly four implementation/documentation files):

- `crates/underwrap-backend/src/main.rs`
- `docs/backend.md`
- `docs/admin-ui.md`
- `docs/remote-backend.md`

This plan changes separately only to record scope and evidence.

Non-goals:

- no recovery HTTP/API/SPA route, unauthenticated browser form, network principal, bootstrap reopening, existing-user overwrite/password reset, last-user-guard bypass, role model, session behavior, rate-limit behavior, password policy, interactive terminal masking, registry lock, or concurrent online recovery;
- no service-manager/container implementation, service account/path default, packaging, NixOS module/service, storage schema, CLI argument, logging, authentication code, dependency, protocol, OpenAPI, generated, backend runtime, frontend, or wrapper-chain change;
- no manual hash generation/editing, sentinel deletion, user-registry deletion, password example/value, secret-bearing command line, later finding, Opportunity, or unresolved item.

Validation commands and evidence:

- `cargo test -p underwrap-backend cli`
- `cargo test -p underwrap-backend --bin underwrap-backend`
- render `underwrap-backend admin recover-user --help` and require stdin, non-overwrite, configured per-user registry, and unchanged-sentinel wording without a state-file append claim
- direct documentation review that the normative procedure and both links resolve, require the service identity and complete writer quiescence, and contain no password value, unsafe argv/environment handling, unsafe manual edit, or online-concurrency suggestion
- `cargo fmt -- --check`
- backend all-target Clippy with warnings denied and the exact Nix `web-build` artifact
- `git diff --check`

- [x] Revalidate F-034 against the current CLI, user-registry persistence, tests, admin/remote/backend docs, and historical report/handover. Evidence: direct source and documentation trace at clean HEAD on 2026-08-27; the command exists and the documentation gap plus inaccurate single-file help wording remain.
- [x] Obtain fresh independent approval of the exact normative owner, procedure, boundary, allowlist, non-goals, and static validation before cleanup discovery. Evidence: initial reviews required an exact four-file implementation/docs allowlist, transient plaintext-memory accuracy, complete writer quiescence, and separation of CLI-help from service-identity validation; the plan incorporated every correction and fresh re-review passed on 2026-08-27; no commands were run.
- [x] Complete finding-local and repository-wide cleanup discovery; independently process any accepted cleanup before documentation correction.
  - Finding-local discovery found no independent deletion: the recover-user helper, parser case, distinct-user/non-overwrite tests, service-identity guidance, and current documentation context are live behavior or acceptance evidence.
  - Repository-wide bounded definition/reachability discovery found no high-confidence private deletion independent of F-034 and the remaining findings.
  - Normative-procedure consolidation, links, and CLI-help correction are the fix; broader documentation cleanup remains F-051 or its eventual current owner.
  - Cleanup is exhausted with no accepted unit and no cleanup commit; the approved documentation correction may begin.
- [x] Write and independently review the normative recovery procedure, links, and accurate CLI help without changing executable behavior.
  - Fix commit `5a666f6155e29eb56fc41f63a93fff793c02efa6` (`docs: explain offline operator recovery`) makes `docs/backend.md` the sole complete procedure, adds concise Admin UI and remote-deployment links, and corrects the single-state-file CLI help claim.
  - The procedure requires the active config, backend service identity, stopped listener and all registry writers, one stdin password line, distinct normalized username, no overwrite/reset, unchanged sentinel, ordinary login/account repair, verification, and safe optional recovery-user deletion.
  - It accurately distinguishes transient plaintext process memory from persisted Argon2id hash and rejects argv/environment/config/history exposure, sentinel/registry deletion, and manual hash edits.
  - Fresh operations and security/scope reviews passed the exact four-file implementation after a separate fix-worker corrected one-sentence-per-line CLI prose.
- [x] Validate, run controller scope/security/diff checks, commit coherently, and record F-034 completion and cross-finding effects before promoting F-035.
  - Direct validation passed 41 focused backend library tests, 3 backend-binary tests, rustfmt, rendered help/content/link assertions, and backend all-target Clippy with warnings denied using exact Nix web artifact `/nix/store/xb0jgmn7ylhdfh78bjw5jgwbc42prrks-underwrap-web-dist-0.1.0` through `UNDERWRAP_WEB_DIST`.
  - Rendered recover-user help hash is `a39a1343187ebd3341536c46aac9bea34b2572d4019cae4464bec4c649aa081d` and contains the configured per-user/non-overwrite/stdin/sentinel contract without append or single-state-file wording.
  - Controller check: exact four-file allowlist and staged checksum `ce005e13c105e460ad3bfabdb8b2b7f93df8928560e60fbec98bc7f73ccfe446`; no executable, authentication, session, API/SPA, bootstrap, registry, password-policy, log, dependency, protocol, OpenAPI, Nix, generated, secret-value, later-finding, or unrelated change.
  - Final clean-tree review passed implementation and scope, then correctly withheld closure until this plan recorded exact Nix web-artifact provenance; fresh evidence re-review is required below.
  - F-034 is resolved; F-035 and F-051 remain unchanged, no other F-id is affected, and no live recovery drill is claimed from static/source/test evidence.
  - Fresh evidence re-review passed the corrected artifact provenance and completed record on 2026-08-27.
  - F-035 is the next medium finding.

### Completed iteration — F-035 wrapper resource-name vocabulary

F-035 remains valid but narrowed at current HEAD `0e332f338fc9c09aaf688b07c24dcbd0ceaa71ac`.

`docs/configuration.md` already calls `wrappers.<name>.resource` an operator-facing backend resource name, and runtime DTOs carry it as `resource_name` before the backend resolves it to a generated stable `res_<hex>` id.

The Nix option description and Rust `WrapperDef.resource` doc comment still call this value a backend resource id, and the VM fixture map comment likewise labels its operator-facing names as ids.

Security boundary:

- Protected asset: correct operator understanding of the wrapper/resource authorization binding and the intended backend resource selected by trusted frontend configuration.
- Failure source: a trusted operator pasting a generated backend id where configuration requires an operator-facing resource name, which can fail closed or create an unintended named resource when wrapper-asset auto-create is enabled.
- Boundary direction: Nix or daemon wrapper configuration `resource` string → daemon runtime `resource_name` registration/open/finalize → backend name resolution → stable resource id used by pairs, permissions, storage, and audit metadata.
- Preserved invariant: terminology changes only; the config key, string value, validation, registration, name resolution, generated ids, permissions, storage keys, wire fields, and runtime behavior remain byte-for-byte unchanged.

Exact scope:

- Change the `services.underwrap.instances.<instance>.wrappers.<wrapper>.resource` Nix option description to **operator-facing backend resource name** and state that the backend resolves it to a stable generated id.
- Apply the same terminology to the Rust `WrapperDef.resource` doc comment.
- Correct the VM fixture comment to say its wrapper map contains operator-facing resource names and that helper code resolves them through `LOCAL_RESOURCE_IDS` before id-addressed admin operations.
- Preserve the already correct `docs/configuration.md` table as the normative configuration wording without editing it.
- Leave legitimate uses of **resource id** intact for generated `res_<hex>` records, pair/permission references, admin paths, storage keys, and backend/runtime responses.

Expected source/documentation allowlist:

- `nix/modules/underwrap.nix`
- `crates/underwrap-protocol/src/config.rs`
- `nix/tests/test.py`
- this plan for evidence

Non-goals:

- no field, option, variable, DTO, serde key, API, OpenAPI, schema, config grammar, validation, resource-name grammar, resource-id generation, auto-create, pair, permission, storage, audit, logging, UI, or runtime behavior change;
- no rename of `resource`, `resource_name`, `resource_id`, `LOCAL_WRAPPER_RESOURCES`, or any backend/admin path;
- no correction of legitimate id terminology, broad vocabulary sweep, F-043 dead-field cleanup, F-051 general documentation cleanup, later finding, Opportunity, or unresolved item.

Validation commands and evidence:

- `cargo test -p underwrap-protocol config`
- parse `nix/tests/test.py` with Python `ast.parse` without writing bytecode
- static positive checks for **operator-facing backend resource name** in the Nix and Rust option comments and **resource name** in the VM fixture comment
- static negative checks that the three stale contexts no longer label configured names as ids, while generated-id references remain present
- `cargo fmt -- --check`
- `nix flake check --all-systems --no-build`
- `git diff --check`

- [x] Revalidate F-035 through the current option/config docs, runtime DTO and backend name-resolution flow, generated-id model, VM fixture use, and historical report/handover. Evidence: direct CodeGraph/source/documentation trace and focused repository terminology search on 2026-08-27; exactly three stale comments remain and published configuration wording is already correct.
- [x] Obtain fresh independent approval of the exact three-comment scope, authorization boundary, legitimate-id exclusions, allowlist, non-goals, and static validation before cleanup discovery. Evidence: security review passed; scope review found one incorrect `WrapperConfig` symbol name, the plan corrected it to `WrapperDef.resource`, and fresh re-review passed on 2026-08-27; no commands were run.
- [x] Complete finding-local and repository-wide cleanup discovery; independently process accepted cleanup before terminology correction.
  - Finding-local discovery found no independent deletion: all three comments attach to live configuration or fixture logic, and renaming fields/variables would add churn or change excluded contracts.
  - Repository-wide bounded reachability discovery found no high-confidence private production deletion independent of the remaining findings.
  - Comment correction is the fix; tests, generated surfaces, F-043 cleanup, broader vocabulary work, renames, formatting, and cosmetic churn are rejected.
  - Cleanup is exhausted with no accepted unit and no cleanup commit; the approved terminology correction may begin.
- [x] Correct and independently review the three comments without changing executable behavior.
  - Fix commit `5f9e6d6cac91685d0e22d8be4bb4b02d7be121d5` (`docs: distinguish resource names from ids`) corrects the Nix option description, `WrapperDef.resource` Rustdoc, and `LOCAL_WRAPPER_RESOURCES` fixture comment.
  - The initial writer changed the already accurate `LOCAL_RESOURCE_IDS` comment instead of the stale wrapper map; a separate fix-worker restored it verbatim and corrected the intended context.
  - Fresh review then rejected hyphenated `resource-name`/`stable-id` wording; a separate fix-worker applied the canonical **resource name**/**resource id** terms, and fresh re-review passed.
  - No executable identifier, value, field, serde/config key, DTO, API, schema, generated artifact, or legitimate stable-id comment changed.
- [x] Validate, run controller scope/diff checks, commit coherently, and record F-035 completion/cross-effects before promoting F-036.
  - Direct validation passed 20 protocol configuration tests, Python AST parsing without bytecode, exact positive/negative terminology checks, rustfmt, all-system Nix evaluation, and diff checks.
  - Controller check: exact three-file allowlist and staged checksum `c9b3ca05230a9b192453bfeae6aaa1f488199e81bf0268f0b300127138b66331`; only documentation/Rustdoc/Python comments changed and generated-id references remain present.
  - Fresh clean-tree review independently reproduced the exact commit/parent, three contexts, data flow, checksum, no-behavior scope, and returned PASS.
  - F-035 is resolved; already correct published configuration wording remains unchanged, F-043 and F-051 remain separate, and no other F-id is affected.
  - Fresh final evidence/process re-review passed this completed record on 2026-08-27.
  - F-036 is the next medium finding.

### Completed iteration — F-036 close-control keyboard focus

F-036 remains directly reproducible at current HEAD `b23638d9c596d7e1e9f1dc49e0ca22613ebac2ff`.

The shared dialog and sheet close buttons still use `focus:outline-hidden` without a visible regular-color replacement, while shared buttons, inputs, selects, and textareas use `focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-(--color-accent)/40`.

Tailwind's compiled `outline-hidden` utility emits a two-pixel transparent outline inside `@media (forced-colors: active)`, preserving a system-adjusted outline when ordinary box-shadow rings are suppressed.

Security boundary: none.

Accessibility impact:

- Affected user: a keyboard operator locating the active close button in every admin dialog or mobile navigation drawer.
- Boundary direction: keyboard focus modality → Radix close control → theme/forced-colors CSS focus indicator.
- Preserved behavior: focus order, focus trapping/restoration, auto-focus, Escape, pointer interaction, close callbacks, dialog/sheet structure, themes, routing, and backend requests remain unchanged; only the visible focus styling and its regression evidence change.

Exact scope:

- Replace `focus:outline-hidden` on each `DialogPrimitive.Close` rendered by `dialog.tsx` and `sheet.tsx` with the exact shared `focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-(--color-accent)/40` treatment.
- Extend the existing ConfirmDialog component test to select the dialog close button by exact accessible name `Close`, preserve its close interaction assertion, require the exact three shared focus-visible classes, and require absence of superseded `focus:outline-hidden`.
- Extend the existing AppShell drawer test with the same static class/name and preserved close-interaction assertions for the sheet close button.
- Keep jsdom evidence limited to rendered semantics and class tokens; it does not prove browser keyboard focus, theme rendering, or forced-colors behavior.
- Add one desktop Playwright spec that logs in and uses keyboard input to open the real **Create store** dialog, confirms its `Name` field is auto-focused, and uses `Shift+Tab` to focus the exact `Close` button without clicking or calling `focus()`.
- In the application's actual dark and light states controlled by the `<html>` `dark` class/theme toggle, require the desktop close button to have focus, match `:focus-visible`, change from an unfocused `box-shadow: none` baseline to a non-`none` computed box shadow, and make no contrast claim.
- Activate Playwright forced-colors emulation, re-enter the same keyboard focus path, and require `:focus-visible`, computed outline style other than `none`, and computed outline width of at least two pixels without requiring a box-shadow ring or claiming contrast.
- In the same desktop-1280 Chromium spec, add a second scenario that sets the page viewport to 375×812, opens the responsive **Open navigation** sheet by keyboard, reaches its exact `Close` button through keyboard navigation without click/direct focus, and applies the same regular-color box-shadow and forced-colors outline assertions.
- Record only the observed two-control, configured Chromium project, desktop/375-pixel responsive layouts, application-theme, and forced-colors evidence in `docs/testing.md` without claiming general WCAG conformance, contrast, screen-reader behavior, mobile-browser support, cross-browser support, or application-wide focus coverage.

Expected source/test/documentation allowlist:

- `web/src/components/ui/dialog.tsx`
- `web/src/components/ui/sheet.tsx`
- `web/src/components/confirm-dialog.test.tsx`
- `web/src/components/app-shell.test.tsx`
- new `web/e2e/focus-indicators.spec.ts`
- `docs/testing.md`
- this plan for evidence

Non-goals:

- no global focus utility, Tailwind/theme token, accent color, shared button/input/select/textarea, dialog/sheet primitive, focus-trap/order/restoration, auto-focus, animation, size, placement, opacity, icon, accessible name, close behavior, routing, backend, API, or data-flow change;
- no visual snapshot baseline, automated contrast calculation, general accessibility/WCAG certification, screen-reader claim, browser matrix beyond the configured Playwright browser, tablet coverage under F-053, F-037 routing work, later finding, Opportunity, or unresolved item;
- no dependency, package-lock, generated API/schema, committed `web/dist`, Rust, Nix, privileged-chain, or security-boundary change.

Approved cleanup 1 — prune unreachable private primitive exports:

- In `web/src/components/ui/dialog.tsx`, delete the definition/export of unconsumed `DialogTrigger` and `DialogClose`, and remove only the exports of internally retained `DialogPortal` and `DialogOverlay`.
- In `web/src/components/ui/sheet.tsx`, delete the definition/export of unconsumed `SheetClose`.
- Preserve `Dialog`, `DialogContent`, every internally used portal/overlay component, and every sheet export with a live consumer.
- Repository-wide symbol/import/dynamic-module search finds no external consumer; `underwrap-web` is private with no package entry point, and Nix ships only the compiled static SPA.
- This independently reversible cleanup removes three dead aliases and five misleading private exports before the same files receive focus styling; it changes no rendered element, class, interaction, test, route, artifact, or F-id.
- Cleanup allowlist: `web/src/components/ui/dialog.tsx` and `web/src/components/ui/sheet.tsx` only.
- Cleanup validation: focused ConfirmDialog/AppShell tests, full `npm test`, `npm run build`, exact symbol absence/retained-use checks, generated-artifact cleanup, and `git diff --check`.

Validation commands and evidence:

- `cd web && npm test -- src/components/confirm-dialog.test.tsx src/components/app-shell.test.tsx`
- `cd web && npm test`
- `cd web && npm run build`
- build `underwrap-backend` with the exact Nix `web-build` artifact through `UNDERWRAP_WEB_DIST`, then run `cd web && npm run build` before direct Playwright commands unless `UNDERWRAP_BACKENDD_BINARY` supplies the exact backend binary
- inside `nix develop`, run `npx playwright test e2e/focus-indicators.spec.ts --project=desktop-1280`
- inside `nix develop`, run the complete `npm run test:e2e`
- inspect built CSS once for the shared focus-visible ring utilities and forced-colors `outline-hidden` rule; source/component assertions prove both close controls apply them
- remove generated `web/dist` and confirm no generated schema, Playwright output, trace, screenshot, or report remains
- `git diff --check`

- [x] Revalidate F-036 against both close-control sources, shared control styling, theme tokens, Tailwind forced-colors output, current component/browser tests, and historical report/handover. Evidence: direct CodeGraph/source/test/build-CSS inspection on 2026-08-27; both controls still suppress regular focus and no current test covers their indicator.
- [x] Obtain fresh independent approval of the exact shared classes, two component tests, two desktop-project browser scenarios, accessibility impact, allowlist, non-goals, and validation before cleanup discovery. Evidence: initial reviews required exact computed-style baselines, proof-layer separation, real theme/keyboard flows, forced-colors refocus, built-CSS precision, browser prerequisites, and constrained claims; a follow-up caught that `mobile-375` uses WebKit, so the plan moved the 375×812 sheet scenario into desktop-1280 Chromium. Fresh final re-review passed on 2026-08-27; no commands were run.
- [x] Complete finding-local and repository-wide cleanup discovery; independently process accepted cleanup before focus correction.
  - Finding-local discovery accepted the exact five-symbol private-export cleanup above; independent reachability/value review returned ACCEPT with no dynamic or package consumer.
  - Repository-wide bounded discovery found no other high-confidence private production deletion independent of remaining findings.
  - Focus classes, component/browser tests, built-CSS evidence, and testing documentation are the fix; broader primitive refactors, test deletion, global styling, later findings, formatting, and cosmetic churn are rejected.
  - Cleanup is exhausted after cleanup 1.
- [x] Implement, independently review, validate, and commit cleanup 1 before the focus correction.
  - Commit: `230c0c91bf201dd02ce03ac367f3537163a71aca` (`chore: prune unused dialog primitive exports`).
  - Exact staged diff: two approved files, eight deletions, SHA-256 `adb6039a72dcc9bcb3f26b7fe4979cc6e902867a49dc97f774f7ee0d4a3470ab`; it removes only the three unreachable aliases and five exports, retaining internal portal/overlay definitions, live exports, rendered close controls, classes, and interactions.
  - Validation: focused ConfirmDialog/AppShell suite passed 8 tests (log SHA-256 `f407cfbe6bc1113ebec0ef91de930973ca0b67fc17247daa3d9f8f13b4e34162`); full Vitest passed 130 tests (log SHA-256 `ac957a421eb74285458cded318ce0f4deb04c1e170c17608ab507adfde335eeb`); TypeScript/Vite production build passed with only the existing large-chunk advisory (log SHA-256 `898238094c76dea80a8ed29d9a13b46fb482bfcb94794b58240aa10a18f51fee`).
  - Review: the first post-implementation review confirmed the source but blocked on unavailable direct diff/test evidence and the ignored generated schema; the controller removed the schema, reran every check, and a fresh reviewer passed exact allowlist, reachability, retained behavior, logs, and artifact absence with no gaps.
  - Controller checks: exact two-file status/diff, symbol absence/retained internal use, no test/doc/focus/F-id change, `git diff --check`, no `web/dist`, generated schema, Playwright output, coverage, traces, or screenshots, and clean post-commit tree all passed. No security boundary changed.
- [x] Implement and independently review the two close-control styles and focused component/browser evidence without changing interaction behavior.
  - Fix commit: `6edb6b0eafc4be273bd6e7d4191c9a4bd5c31734` (`fix: restore close-control focus indicators`).
  - Each close control replaces only `focus:outline-hidden` with the exact shared `focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-(--color-accent)/40` tokens; every other rendered token, primitive, callback, accessible name, focus mechanism, theme, route, and backend flow remains unchanged.
  - ConfirmDialog and AppShell tests select exact `Close`, require all three tokens and absence of the superseded token, and exercise the existing close callbacks/removal.
  - The new desktop-1280 Chromium spec reaches both controls without click/direct focus: Create store proves `Name` auto-focus then `Shift+Tab` in actual dark and toggle-driven light themes; the 375×812 scenario reaches the responsive navigation's first link then `Shift+Tab`. Each regular-color path proves an unfocused `box-shadow: none` baseline and a focused `:focus-visible` non-`none` shadow; each forced-colors path reopens/refocuses and proves a non-`none` outline at least two pixels wide.
  - Fresh implementation review passed the six-path diff and non-vacuous browser proof; a broader evidence review exhausted its deadline without a verdict, then a bounded fresh review passed accessibility scope, Tailwind semantics, documentation claims, Chromium project identity, and non-overlap. Final post-validation re-review passed all source, tests, logs, artifacts, and exclusions with no gaps. No correction worker was required.
- [x] Validate, run controller scope/diff/artifact checks, commit coherently, and record F-036 completion/cross-effects before promoting F-037.
  - Exact staged fix diff: six approved paths, 215 insertions and three deletions, SHA-256 `856a4f0c2c3a3e5da0c2ea3d51f1439c04f69f831b898785b31e66cb6059e85e`.
  - Vitest: focused ConfirmDialog/AppShell passed 8 tests (log SHA-256 `f6ece5a92a989fdd8314c1fbf936ca680248cd0c2fed4407ac9e5e132daad5bb`); the full suite passed 130 tests (log SHA-256 `b336e447e470e8fe3a5dfecd4debd66b43634f5f72e24058e8d57d9200763161`).
  - Build/browser prerequisites: TypeScript/Vite production build passed with only the existing large-chunk advisory (log SHA-256 `682648d811f5981fc8110b3f0a18d5ec2a2280a9ab177ac83282e368a05833f3`); the exact Nix `web-build` artifact at `/nix/store/ym0f3mz64a7kbaxzy1j5fd3995pdvg6b-underwrap-web-dist-0.1.0` supplied `UNDERWRAP_WEB_DIST` for a successful backend build (log SHA-256 `34dc9cf5f0e61d0d1dfef660a14ce1d6394b51e199285548e86edc38eabdee84`).
  - Playwright: the focused configured-Chromium run passed both scenarios (log SHA-256 `0ad254188c3b25015f02a3610fb0af621e7c7d88385417e1736f9d2f8db72bd4`); the complete configured suite passed 33 tests, including all desktop and existing mobile-project scenarios (log SHA-256 `a05c6622316bbb406e0c91510e89e6268963c7e4c59bfc967881137ca772ea38`).
  - Built CSS contains the three shared focus-visible utilities and Tailwind's forced-colors two-pixel transparent-outline rule (log SHA-256 `443fb34646df54a73316d8cb324b030ea0f2a392635d55bebdeb76e48bf085e2`).
  - Controller checks passed exact six-path scope, exact two one-line production substitutions, keyboard-only close traversal, no direct close focus/click, docs non-claims, `git diff --check`, and absence of generated schema, `web/dist`, Playwright results/report/traces/screenshots, coverage, dependency/config/lock changes, generated API changes, later-finding changes, and security-boundary changes; post-commit tree was clean.
  - F-036 is resolved for the two shared close controls in the observed configured Chromium desktop and 375×812 responsive flows. No contrast, WCAG, screen-reader, mobile-browser, cross-browser, or application-wide accessibility claim is made.
  - U-006 is narrowed but remains unresolved: its historical claim that no forced-colors gate exists is no longer current for these two configured-Chromium close-control paths, while axe, contrast, zoom/reflow, visual-regression, app-wide forced-colors, cross-browser, and broader accessibility/visual conformance remain unestablished.
  - O-016 remains unresolved: observing this one dialog control in actual dark/light states does not establish complete paired theme tokens or decide the product-wide light-theme posture.
  - F-053 remains fully unresolved because this unit adds no tablet test and changes no Playwright project; F-037 and every other remaining finding are unchanged. F-037 is next.
  - Final closure review first required the U-006 cross-effect above and independent status/hash/stat completion; after correction and controller recomputation, fresh re-review passed the ordered workflow, evidence, bounded claims, cross-effects, and F-037 promotion with no blocker.

### Current iteration — F-037 per-store selection and list states

F-037 remains directly reproducible at clean HEAD `6dfd19b8c138b408aa00fdbcc3d20467952a3e41`.

`useStoreSelector` exposes any non-empty `?store=` value immediately, never reconciles it with a successful store list, and only chooses the first available store when the URL has no value.

Consequently both per-store pages can issue a detail request for a stale/deleted store, render a `<select>` whose value has no option, and remain stranded after the available list changes.

When the initial store-list query fails without data, both pages instead render text about an unavailable per-store grant even though every authenticated operator is a full admin and no per-store operator grants exist.

Security boundary: none; this changes no authentication, authorization, backend policy, resource access, or secret handling.

Operational-correctness flow:

- Authoritative input: the successfully resolved admin store list.
- Untrusted/stale input: the optional browser URL `store` query value.
- Boundary direction: URL-requested id + store-list state → validated effective selection → store-specific query and selector/URL state.
- Preserved behavior: valid URL selection, first-store defaulting, operator selection, unrelated query/hash state, detail-query error handling, store ordering, and all backend authorization remain unchanged.

Exact scope:

- Keep a URL-requested store id in hook state, but return no effective `storeId` while the store list is unresolved or unavailable so no store-specific query can run before validation.
- On every successful list value, derive the effective id synchronously: preserve a requested id that exists, otherwise choose the first item, or return `null` for an empty list.
- Reconcile hook state and the URL after derivation: replace a missing/stale/deleted id with the effective first id, delete `store` after a successful empty list, preserve unrelated query parameters and the hash, preserve `window.history.state`, avoid no-op replacements, and create no history entry.
- Keep `setStoreId` immediate for valid selector choices and preserve all other URL state.
- Listen for same-mounted-page `popstate` changes, reread the requested URL id, and pass it through the same validation/canonicalization path; do not intercept navigation or write from the listener itself.
- On every effective-store change, synchronously suppress every open store-scoped dialog before it can receive the fallback store id, then reset the page's reveal/edit/create/delete/raw-view transient state; never combine an old resource, pair, wrapper, or secret address with the new store.
- In each page's no-effective-selection branch, render store-list loading first, then `describeApiError(stores.error)`, then an accurate successful-empty warning that no stores are configured and the operator must create one on the Stores page.
- Remove every grant/access/visibility diagnosis from those successful-empty branches; all authenticated operators retain the documented equal authority.
- Keep store-specific loading/error/content/empty states unchanged after a valid effective selection.
- Add deterministic StrictMode hook tests for unresolved stale input, successful fallback, selected-store removal, successful empty-list URL clearing, valid selection/setter behavior, URL/hash/exact-history-state preservation, no new history entry, popstate revalidation, exactly one required canonicalization, and zero `replaceState` calls for an already canonical render or already-effective setter choice.
- Add one cross-page Vitest file that applies the same matrix to Secrets and Wrapper permissions: a pending list never triggers a stale detail request; resolution selects only the valid first store and corrects selector/URL; a list error preserves the requested URL and renders the network/API error without grant/content-empty text or a detail request; and a successful empty list removes only `store`, renders accurate create-store guidance, and triggers no detail request.
- For each page, start from a valid secondary selection with an open create dialog, remove that store from the resolved query data, and prove the dialog is synchronously closed/reset, selector and URL fall back to the first valid store, no post-removal request names the deleted id, and the fallback detail query alone runs.
- Add one configured desktop-1280 Chromium spec with three data-driven scenarios across both pages, with request observations reset after login and scoped to exact detail endpoints: a real backend stale URL falls back to real fixture store `default`, renders successful default content, makes at least one default detail request and no stale detail request while preserving unrelated URL state; an intercepted structured store-list failure preserves the requested `store` plus unrelated URL state and renders its error before any detail/empty diagnosis or detail request; and an intercepted successful empty list removes only `store`, preserves unrelated URL state, and renders create-store guidance without a detail request.
- Record this bounded hook/page/browser evidence in `docs/testing.md`.

Expected source/test/documentation allowlist:

- `web/src/hooks/use-store-selector.ts`
- new `web/src/hooks/use-store-selector.test.tsx`
- `web/src/pages/secrets/page.tsx`
- `web/src/pages/wrapper-permissions/page.tsx`
- new `web/src/pages/store-selection.test.tsx`
- new `web/e2e/store-selection.spec.ts`
- `docs/testing.md`
- this plan for evidence

Expected cross-effects:

- Closing F-037 closes only the historical V-006/V-009 stale/deleted-id and store-list-state coverage gap for these two pages.
- O-015 mobile entity cards, F-049 backend response redaction, F-050 Playwright fixture cleanup, F-053 tablet discovery, F-058 tool refresh, authentication/authorization, every security boundary, and every other finding/Opportunity/unresolved item remain unchanged.

Non-goals:

- no backend/API/OpenAPI/schema, store CRUD/order/id grammar, authentication/session, operator/user role, per-store grant, runtime-client permission, resource/secret, or security-boundary change;
- no TanStack route definition/search-schema, query-client retry/cache policy, global error mapping, store-specific error/empty content, URL push/new history entry, fallback toast/banner, store-name copy into the URL, dialog mutation semantics, or router redesign;
- no mobile entity cards or O-015, responsive layout, Playwright project/configuration, mobile/tablet/F-053 coverage, accessibility/theme work, later finding, Opportunity, or unresolved-item implementation;
- no dependency, lockfile, generated API/schema, committed `web/dist`, Rust, Nix, privileged-chain, backend-binary, or VM change.

Validation commands and evidence:

- `cd web && npm test -- src/hooks/use-store-selector.test.tsx src/pages/store-selection.test.tsx`
- `cd web && npm test`
- `cd web && npm run build`
- `system=$(nix eval --impure --raw --expr builtins.currentSystem); web_dist=$(nix build --no-link --print-out-paths ".#checks.${system}.web-build"); UNDERWRAP_WEB_DIST="$web_dist" cargo build -p underwrap-backend`, then use `target/debug/underwrap-backend` and retain local `web/dist` only while running browser tests
- inside `nix develop`, run `npx playwright test e2e/store-selection.spec.ts --project=desktop-1280`
- inside `nix develop`, run the complete `npm run test:e2e`
- inspect focused browser request evidence for zero stale/failed/empty store-detail requests and exact URL preservation/canonicalization
- remove generated `web/dist`, schema, Playwright output, trace, screenshot, report, and coverage artifacts
- `git diff --check`

- [x] Revalidate F-037 against the current hook, both page branches/detail-query gates, equal-admin documentation, unit/browser coverage, router/query configuration, and historical report/handover. Evidence: direct CodeGraph/source/test/history inspection at clean HEAD on 2026-08-30; stale ids and inaccurate list-failure/empty diagnoses persist, and existing tests cover only valid/normal page flows.
- [x] Obtain fresh independent approval of the synchronous validated-selection and transient-dialog reset design, popstate/URL/no-op semantics, equal-admin copy, exact test matrix, allowlist, non-goals, cross-effects, and validation before cleanup discovery. Evidence: initial reviews required synchronous suppression/reset of old-store dialogs, explicit no-op replacement evidence, per-page selected-store-removal coverage, phase-scoped exact browser requests, real-backend default fidelity, complete URL/history assertions, cross-effect ownership, and an exact Nix prerequisite command. The plan incorporated every correction; fresh implementation-scope and test/evidence re-reviews passed on 2026-08-30 with implementation/runtime evidence pending.
- [x] Complete finding-local and repository-wide cleanup discovery; independently process accepted cleanup before selector/error correction.
  - Finding-local review found the requested-id state, both selectors, every Secrets and Wrapper-permissions transient state, policy viewer, query hooks, and render branches live; no independent net deletion exists.
  - Hook restructuring/comments, page-branch consolidation, transient-state reset, tests, browser instrumentation, and documentation are the F-037 fix rather than cleanup.
  - Repository-wide bounded private-definition/reachability review found no high-confidence dead production deletion independent of current or later findings.
  - Broad page extraction, test/doc deletion, routing refactors, generated files, formatting, dependency work, later findings, and cosmetic churn are rejected.
  - Cleanup is exhausted with no accepted unit and no cleanup commit; the approved selector/error correction may begin.
- [ ] Implement and independently review the hook, two page branches, unit/browser evidence, and testing documentation without changing authorization or valid-store behavior.
- [ ] Validate, run controller scope/diff/artifact checks, commit coherently, and record F-037 completion/cross-effects before the mandatory medium → low decision.

### Remaining medium queue

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
