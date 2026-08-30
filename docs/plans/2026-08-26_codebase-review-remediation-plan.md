# Complete the accepted codebase-review remediation

Status: Active

Execution authority: On 2026-08-27 the user explicitly selected **Transfer and continue**, carrying D-174's standing authority into this plan through F-058 with every stop condition and severity gate intact.

Severity transition: After evidenced completion of F-031 through F-037, on 2026-08-30 the user selected **Approve transition** with no comment in the required focused medium → low prompt. This opens the low band and authorizes F-038 revalidation/scoping next under the unchanged one-finding, cleanup-first, independent-review, validation, and stop-gate protocol; it does not approve a later finding, Opportunity, unresolved item, dependency, architecture/product choice, binary-chain change, or weaker security boundary.

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

No low finding was promoted by this survey. F-032 through F-037 were later completed with current evidence, and the user-approved medium → low decision recorded above now permits F-038 revalidation/scoping.

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

### Completed iteration — F-037 per-store selection and list states

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
- [x] Implement and independently review the hook, two page branches, unit/browser evidence, and testing documentation without changing authorization or valid-store behavior.
  - Fix commit: `eb8352c05037f444ca842679d98ff3811f7f0cc1` (`fix: reconcile per-store route selection`).
  - The hook now distinguishes requested URL state from a synchronously validated effective id, returns `null` until a list resolves, preserves a valid id, otherwise chooses the first current store or clears an empty selection, and never exposes stale ids to detail-query gates.
  - URL canonicalization preserves exact history state, unrelated query/hash state, and history length; it rejects invalid setter ids, avoids canonical/effective no-op replacements, collapses duplicate parameters, revalidates same-path popstate, and ignores a popstate for a different mounted pathname.
  - Both pages render loading, actual store-list errors, and successful no-store guidance in that order. Equal-admin authority remains unchanged and no text diagnoses a nonexistent grant/access restriction.
  - Effective-store changes synchronously gate and then reset every top-level transient. Secrets additionally keys each resource subtree by `(storeId, resource_id)` so cached same-id resource/libsecret state and destructive dialogs cannot rebind; Wrapper permissions also store-keys its persistent raw-policy viewer.
  - Initial broad implementation reviews exhausted their deadlines; three bounded reviews found invalid-setter and duplicate-popstate gaps, nested same-resource libsecret rebinding, insufficient reset re-arming/destructive-dialog evidence, and cross-navigation browser-request attribution. A separate correction worker fixed every accepted issue. Three fresh focused re-reviews passed hook, page/nested-state, browser, and documentation behavior; the final post-validation review passed the exact seven-path diff with no remaining correction or gap.
- [x] Validate, run controller scope/diff/artifact checks, commit coherently, and record F-037 completion/cross-effects before the mandatory medium → low decision.
  - Exact staged diff: seven approved paths, 1,028 insertions and 122 deletions, SHA-256 `bce3425fb0882ed461c38de133c0674dfd2fa9ffa3dac74e0c08fa94e4dc2c84`.
  - Vitest: 9 StrictMode hook and 9 cross-page selection/state tests passed in the focused 18-test run (log SHA-256 `78bfe460419f4864bfd38fc1db0cba82eecfdd8f1e5a0e888857f1b4416d698c`); the complete 148-test suite passed (log SHA-256 `a3a8a4de4d7ca5b64e86b9d408636983af53adb924dde7d47d487d4498b657a4`).
  - TypeScript/Vite production build passed with only the existing large-chunk advisory (log SHA-256 `bb41d0167edb6c6490e413227f2fcd3525683949e968684346a2b8caab205886`).
  - The exact Nix web artifact `/nix/store/fy0zc6mfa7vpz48nas3g7x7qyyfvlvxv-underwrap-web-dist-0.1.0` supplied `UNDERWRAP_WEB_DIST` for the successful `target/debug/underwrap-backend` prerequisite build (log SHA-256 `79550ddef9fa053890cddcfd8edb0f616f3a7dab343e6eadd877391971c1230a`).
  - Playwright: the focused desktop-1280 Chromium spec passed all 3 stale/error/empty scenarios across both pages (log SHA-256 `b709297ed0fcbd994884a054de3a504021b22a63d80ade50f0723ff159744fda`); the complete configured suite passed 36 tests (log SHA-256 `4ccbfa54eaf644ea31ea3659c43d949ee60fe228b86c0822f89825cb79085e42`).
  - Controller checks passed exact seven-path scope, synchronous detail gating, removal of grant/access copy, all top-level and nested transient protections, exact endpoint request assertions, no package/lock/router/Playwright-config/API/backend/auth/security/later-finding change, `git diff --check`, generated/schema/artifact absence, staged checksum, and clean post-commit tree.
  - F-037 is resolved and closes the historical V-006/V-009 stale/deleted-id and store-list-state coverage gap for Secrets and Wrapper permissions only.
  - O-015, F-049, F-050, F-053, F-058, authentication/authorization, every security boundary, every lower finding, and every other Opportunity/unresolved item remain unchanged.
  - F-031 through F-037 are now resolved. The program stopped for the focused mandatory medium → low transition decision before F-038; no low-severity work was started before that decision.
  - Fresh final closure re-review passed the exact commit/scope/stat/checksum, source semantics, retained logs/counts, bounded cross-effects, completed medium band, checklist state, and D-174 gate interpretation; after the plan-only closure commit, `ask_user` was the only authorized next program action.

### Completed medium → low transition

- [x] Obtain and record the mandatory medium → low transition decision. Evidence: after F-031 through F-037 and their closure records were complete at clean HEAD `7f5ae2472e69332a0d6a64cc378936e53f19d4c6`, the focused `ask_user` prompt presented **Approve transition**, **Pause here**, and **Request changes** with comment entry enabled. On 2026-08-30 the user selected **Approve transition** with no comment. F-038 is next; every D-174 stop condition and per-finding gate remains controlling.

### Completed iteration — F-038 JCS-safe resource timestamps

F-038 remains valid but narrowed at clean HEAD `5afb8f4583bacd5aefba411f0d54336f361d9984`.

The resource-blob module states that all four libsecret `u64` timestamps are constrained to the ECMAScript/JCS safe-integer maximum `2^53 - 1`, but `Collection::{created, modified}` and `Item::{created, modified}` still derive unrestricted Serde encoding and decoding.

`ResourceBlob` and `LibsecretSection` canonical methods therefore accept values above the documented v1 contract. F-008 already makes the private host-selectable publication parser reject such values through `validate_publication_semantics`, so the remaining gap is the generic persisted/canonical type boundary and its direct Serde surface.

Consequence: a typed Rust blob can encode an out-of-contract number and other implementations can interpret canonical numeric values differently; ordinary Unix-second timestamps remain many orders of magnitude below the boundary.

Security boundary: none. F-008's untrusted target-publication integrity boundary is already fail-closed and remains unchanged; this iteration repairs the cross-implementation canonical-format contract.

Canonical-format flow:

- Input sources: typed in-memory `Collection`/`Item` values or JSON bytes decoded into those public types.
- Boundary direction: four unsigned timestamp fields → Serde/JCS encode or JSON decode → canonical `LibsecretSection`/`ResourceBlob` → persistence, transport, publication, and writeback consumers.
- Preserved behavior: every value from zero through `9_007_199_254_740_991`, canonical field names/order/bytes, schema version, complete-resource and publication size authorities, publication-only identifier/reference rules, and timestamp-generation/update semantics remain unchanged.
- Newly rejected state: any of the four fields at `9_007_199_254_740_992` or above, including previously persisted out-of-contract v1 blobs; this greenfield correction needs no migration or schema-version bump.

Exact scope:

- Define one private inclusive safe-integer constant and one stable value-free error message in `resource_blob.rs`.
- Implement one private Serde adapter that deserializes as `u64` and rejects above the maximum, and rejects above the maximum before `serialize_u64` on encoding.
- Apply that adapter to exactly `Collection.created`, `Collection.modified`, `Item.created`, and `Item.modified`; retain the public Rust field type `u64` and every wire key.
- Keep `validate_publication_semantics` timestamp checks for direct validation of manually constructed in-memory sections, but make them share the same constant/predicate/message. Do not apply publication-only collection/item/alias/reference restrictions to generic canonical resource methods.
- Preserve deterministic error precedence: complete-resource size before resource parse; private publication body cap before publication parse; timestamp decode before schema-version/canonical/publication semantics; and timestamp encode before any post-serialization size check.
- Correct the module Rustdoc so it states explicit encode/decode rejection rather than claiming above-limit values merely round through a floating-point representation.
- Move only the two existing timestamp-focused tests out of the 625-line `resource_blob/tests.rs` into a new nested `resource_blob/tests/timestamps.rs`, declare that module from `tests.rs`, and keep both resulting test files below the repository's approximate 600-line target. Do not broadly repartition unrelated resource/publication tests.
- In the timestamp module, require the full 32-outcome matrix with the other three timestamps held at safe values: four fields × two containers (`ResourceBlob`, `LibsecretSection`) × typed maximum serialization success, independently formatted raw-JSON maximum decoding success, typed maximum-plus-one serialization rejection, and independently formatted raw-JSON maximum-plus-one decoding rejection.
- Construct every decoding vector as JSON bytes without first serializing an annotated `Collection`, `Item`, `LibsecretSection`, or `ResourceBlob`; the rejecting typed serializer must never be used to manufacture an above-limit input.
- Exercise `validate_publication_semantics()` directly on manually constructed sections: all four fields accept the maximum, temporal relationships such as created greater than modified remain accepted, and each field independently rejects maximum-plus-one without serialization.
- Require the same stable value-free timestamp message in direct semantic, serialization, and deserialization error chains.
- Update the strict-publication regression to prove typed encoding rejects maximum-plus-one and independently supplied above-limit JSON rejects during decode.
- Pin precedence with explicit mixed-defect vectors: an unsafe-timestamp resource above 2 MiB returns `ResourceTooLargeError`; an unsafe-timestamp publication above 1 MiB returns the publication-cap error; within bounds timestamp decode precedes unknown schema version, noncanonical bytes, and publication identifier/reference semantics; and typed maximum-plus-one serialization returns the timestamp error even when the eventual resource would otherwise exceed 2 MiB.
- Document the persisted resource timestamp contract in `docs/backend.md` and the exact four-field encode/decode matrix in `docs/testing.md`; leave the already-accurate publication-specific `docs/secretsd-security.md` rule unchanged.

Expected source/test/documentation allowlist:

- `crates/underwrap-protocol/src/resource_blob.rs`
- `crates/underwrap-protocol/src/resource_blob/tests.rs`
- new `crates/underwrap-protocol/src/resource_blob/tests/timestamps.rs`
- `docs/backend.md`
- `docs/testing.md`
- this plan for evidence

Expected cross-effects:

- F-008 remains resolved; its publication boundary rejects unsafe timestamps earlier during Serde decoding while retaining the independent direct semantic check and all identifier/reference validation.
- F-009's inclusive 2-MiB complete-resource authority and size-before-parse precedence remain unchanged.
- F-039 remains unresolved because timestamp update/propagation semantics do not change.
- F-042 is narrowed but remains unresolved: the daemon's existing typed `ResourceBlob` parse additionally rejects unsafe timestamps, while the inaccurate documentation claim that it treats the blob as opaque still requires its own correction.
- F-040, F-045, every other finding, Opportunity, and unresolved item remain unchanged.

Non-goals:

- no timestamp newtype, signed/float timestamp, field/key/schema-version/OpenAPI/admin DTO/D-Bus type, temporal relation, future-time, created-before-modified, clock source, update propagation, precision, or unit change;
- no generic publication identifier/path/alias/reference validation at the persisted-resource boundary, canonical-byte exactness change, map/order/secret encoding, size cap, transport envelope, storage layout, migration/repair, compatibility branch, error variant/code, or runtime policy change;
- no dependency, Cargo lock/root manifest, backend handler, store implementation, daemon, namespace, secretsd, integration harness, SPA, NixOS module, VM, privileged chain, security boundary, later finding, Opportunity, or unresolved-item implementation;
- no broad test-suite repartition, unrelated test rewrite/deletion, generated artifact, formatting-only churn, or strict-Rustdoc/F-056 work.

Validation commands and evidence:

- `cargo test --locked -p underwrap-protocol resource_blob::tests::timestamps`
- `cargo test --locked -p underwrap-protocol`
- `cargo test --locked -p underwrap-protocol --no-default-features --features local-abi`
- `cargo clippy --locked --all-targets -p underwrap-protocol -- -D warnings`
- `cargo clippy --locked -p underwrap-protocol --no-default-features --features local-abi -- -D warnings`
- `cargo test -p underwrap-backend-store`
- `cargo test -p underwrap-secretsd-store`
- `cargo test -p underwrap-secretsd`
- `cargo test -p underwrap-ns`
- build the exact Nix `web-build` artifact through `UNDERWRAP_WEB_DIST`, then run `cargo test --workspace --exclude integration-tests -- --test-threads=4`
- `./crates/integration-tests/run`
- `cargo fmt -- --check`
- workspace Clippy with warnings denied and the exact Nix web artifact
- `nix build .#checks.x86_64-linux.workspace-test`
- direct source/test inspection that exactly four fields use the adapter, no generic identifier rule moved, both test modules remain below 600 lines, error text is value-free, and no generated/Cargo/OpenAPI artifact changes
- `git diff --check`

- [x] Revalidate F-038 against the current four fields, both canonical container APIs, strict publication validation, callers, tests, documentation, F-008/F-009 effects, and historical report/handover. Evidence: direct CodeGraph/source/test/doc inspection and two focused design/evidence reviews on 2026-08-30; publication is already protected, but generic encode/decode remains unrestricted and the maximum-only test still omits above-bound vectors.
- [x] Obtain fresh independent approval of the four-field Serde design, direct semantic check, precedence, narrow test split/matrix, docs owner, allowlist, non-goals, cross-effects, and validation before cleanup discovery. Evidence: initial reviews approved field-level Serde enforcement but required the explicit 32-outcome matrix, independently authored raw JSON, direct four-field semantic fallback/message evidence, mixed-defect size/schema/canonical/publication precedence, and F-042 narrowing. The plan incorporated every correction; fresh implementation and evidence re-reviews passed on 2026-08-30 with implementation/runtime evidence pending.
- [x] Complete finding-local and repository-wide cleanup discovery; independently process accepted cleanup before timestamp enforcement.
  - Finding-local review found all canonical methods, four fields, semantic checks, size/error helpers, and existing boundary/publication tests live; no independent net deletion exists.
  - Constant/adapter sharing, Rustdoc correction, relocation of the two timestamp tests, new vectors, and backend/testing documentation are the F-038 fix rather than cleanup.
  - Repository-wide bounded private-definition/reachability review found no high-confidence dead production deletion independent of current or later findings.
  - Broad test repartition, test/doc deletion, generated files, dependencies, later findings, formatting, and cosmetic churn are rejected.
  - Cleanup is exhausted with no accepted unit and no cleanup commit; the approved timestamp enforcement may begin.
- [x] Implement and independently review the adapter, annotations, focused tests, and documentation without changing valid canonical bytes or temporal semantics.
  - Fix commit: `3d984ce17877fcac7a4a9bc2132d6ee8d910ee6a` (`fix: enforce JCS-safe resource timestamps`).
  - One private predicate/message and Serde adapter now reject above-limit values on encode/decode; exactly four annotations cover `Collection.created`, `Collection.modified`, `Item.created`, and `Item.modified`, while direct publication semantic validation retains the same guard and all identifier/reference checks.
  - The old maximum-only and strict timestamp tests moved into the new focused module. Its independently authored JSON vectors exercise the exact 32 outcomes, direct semantic fallback, strict publication, no temporal-order rule, value-free errors, and mixed-defect ordering; unrelated resource/publication tests remain in place at 568 lines and the focused module is 368 lines.
  - Initial source/evidence reviews found that direct semantic maximum acceptance exercised only both `created` fields. A separate correction worker added all-four maximum acceptance while preserving a distinct created-greater-than-modified case and the four independent rejection loop. Fresh post-correction review passed the exact five paths, and final staged review passed source, tests, docs, cross-effects, and retained validation evidence with no gap.
- [x] Validate, run controller scope/diff/artifact checks, commit coherently, and record F-038 completion/cross-effects before promoting F-039.
  - Exact staged diff: five approved paths, 430 insertions and 77 deletions, SHA-256 `558f759fa268262fdeea798c98be9ab3f6ab4e2757613c98f9f07f874527ae76`.
  - Protocol validation passed 5 focused timestamp tests, 118 full-profile tests, 62 local-ABI tests, and full/local denied-warning Clippy (aggregate log SHA-256 `5f045bbd7f3e0f80366bbb5e0d081e67652ca01a47b9b08911cef1cb7df7345`).
  - Dependent crates passed 55 backend-store, 24 secretsd-store, 39 secretsd, and 122 namespace tests (log SHA-256 `9100306d7ff006966ead125d5048d9cf000834b2a52575a8f548c28665309d8b`).
  - Bounded non-integration workspace tests passed with exact Nix web artifact `/nix/store/q6j5j105c1y9ilcppwaph4y9j36wirrz-underwrap-web-dist-0.1.0` (workspace log SHA-256 `d808be2dcc768fc51f90f8378f7b7312600832fed52cf201c45f70d6122e4306`; prerequisite log SHA-256 `33fec8fdf10471630c29c0c98e6ebbe21fce3ccb07019bab8ff0fab74378da14`).
  - The integration runner passed all 105 tests (log SHA-256 `cf9585afd6201915f7e0a1baa6f0c69cdb774ad3ed49b152b4873c608e020831`); rustfmt and full workspace denied-warning Clippy passed (log SHA-256 `55730fd0a6f3564d01c072043ced86bc25b8c397ebd5d54da6bf725b0c3726ed`).
  - The first Nix workspace-test build correctly failed before compilation because Git-flake source filtering omitted the then-untracked new timestamp module declared by `tests.rs` (failure log SHA-256 `ef40fd0644593700e501ec6cff1761e1e1f6ba327b0e3fcd92780494ceac128e`). After exact five-path staging, the same check included the module and passed (log SHA-256 `50239b219b2b6b40b2395cdd0e94bfbe5ac158df9757e955b21ae06769cb3f01`); final review classified the first result as source-inclusion provenance, not an implementation/test failure.
  - Controller checks passed exact five-path scope, four-field/annotation inventory, direct semantic guard, no publication-rule movement, value-free diagnostic, both test files below 600 lines, mixed precedence, no Cargo/dependency/schema/OpenAPI/generated/security/later-finding change, `git diff --check`, staged checksum, and clean post-commit tree.
  - F-038 is resolved for every Serde and canonical-container encode/decode path. Valid canonical bytes and all ordinary timestamps are unchanged; above-limit v1 state is intentionally unreadable/unwritable under the already documented greenfield contract.
  - F-008 and F-009 remain resolved and unchanged at their boundaries; F-039 remains unresolved; F-042 is narrowed because daemon parsing gains this rejection while its inaccurate opaque-blob documentation remains; every other finding/Opportunity/unresolved item is unchanged. F-039 is next.
  - Final closure review found no semantic/evidence correction, requested controller-only Git/hash verification, and then passed after exact plan-only status, five-path 430/77 stat, patch checksum, all retained log hashes, and `git diff --check` were mechanically reconfirmed. F-039 promotion is approved under the unchanged protocol.

### Completed iteration — F-039 containing-collection modification timestamps

F-039 remains directly reproducible at clean HEAD `336e038e38cab12655e7a0d516b3a1ba28d9475f`.

`SecretStore::set_attributes`, `SecretStore::set_label`, and the `Item.SetSecret` handler update `Item.modified` but leave the containing `Collection.modified` stale, while item create, replace, and delete already stamp the collection.

Consequence: the Secret Service `Collection.Modified` property cannot consistently represent accepted item secret/attribute/label mutations. Its Unix-second granularity still permits two mutations in one second to retain the same numeric value; the contract is one sampled assignment, not strict monotonic increase.

Security boundary: none. Target D-Bus input remains untrusted for robustness and storage integrity, but F-012 already isolates every mutation in a candidate and commits only after exact ACK; this iteration corrects standards-visible metadata within that existing fail-closed boundary.

Mutation flow:

- Input: one existing item secret/content-type, attributes, or label mutation.
- Boundary direction: D-Bus item handler → one candidate-store item setter → item plus containing-collection timestamp assignment → canonical publication → exact ACK → accepted live store and `Item.Modified`/`Collection.Modified` getters.
- Preserved behavior: secret zeroization, candidate clone/publish/ACK commit, rejection/transport no-effect, missing-item no-op-success behavior, setter return types, item/collection `created`, arbitrary labels/content types/attributes/secret bytes, and create/replace/delete semantics remain unchanged.

Exact scope:

- Add one private `SecretStore` item-update helper in `store/items.rs` that resolves an existing collection and item, returns `false` with no clock read or state change when either is missing, runs one infallible field mutation, samples `now_secs()` exactly once, and assigns that same value to both `Item.modified` and `Collection.modified`.
- Route `set_attributes` and `set_label` through the helper.
- Reintroduce one narrow public `set_secret(collection, id, secret, content_type) -> bool`, justified by its new production caller, and route it through the same private helper; add no zeroization dependency to `underwrap-secretsd-store`.
- In `Item.SetSecret`, preserve the current candidate's old-secret zeroization before replacement, then call the narrow store setter instead of duplicating field and clock logic. Continue ignoring the boolean result so stale/missing item-interface behavior does not change in this finding.
- Do not clamp or manufacture timestamps; normal `now_secs()` remains far below F-038's canonical maximum, and any future out-of-range candidate remains rejected by canonical publication.
- Extend store tests for all three setters: backdate item/collection `modified` to distinct safe baselines, mutate one field set, require the requested value change, exact item/collection timestamp equality above the baseline, unchanged `created` and unrelated data, and false/no-state-change behavior for missing collection/item.
- Extend D-Bus item tests with accepted ACK cases for secret, attributes, and label. For each, require exact item/collection timestamp equality above a backdated baseline in both the published generation and committed live store, equality of accepted and published metadata, unchanged `created`/unrelated fields, the requested value change, and `Item.Modified` returning the accepted value.
- Strengthen all three existing REJECT tests to require that the published candidate stamps both timestamps equally while the accepted live store remains structurally unchanged; retain one EOF/shared transport failure as representative F-012 evidence.
- Add one table-driven collection-interface test covering secret, attributes, and label on fresh backdated stores; for each public store setter, require `Collection.Modified` to return the exact shared item/collection timestamp. Do not add property-change signals or a new D-Bus wire harness.
- Document the same-sample candidate/ACK behavior and same-second limitation in `docs/secretsd-security.md`, explicitly distinguishing arbitrary loaded timestamps (which retain no imposed relation) from generated successful item mutations (which assign one sample to both modified fields without monotonicity), and record the deterministic backdated setter/ACK/REJECT/property matrix in `docs/testing.md`.

Expected source/test/documentation allowlist:

- `crates/underwrap-secretsd-store/src/store/items.rs`
- `crates/underwrap-secretsd-store/src/store/tests.rs`
- `crates/underwrap-secretsd/src/dbus_service/item.rs`
- `crates/underwrap-secretsd/src/dbus_service/item/tests.rs`
- `crates/underwrap-secretsd/src/dbus_service/collection/tests.rs`
- `docs/secretsd-security.md`
- `docs/testing.md`
- this plan for evidence

Expected cross-effects:

- F-012 remains resolved: rejected, EOF, wrong-response, and publication failures preserve the accepted live store; candidate/displaced secret zeroization and commit-after-ACK remain unchanged.
- F-038 remains resolved: generated Unix-second timestamps use the same `u64` fields and canonical safe-integer enforcement.
- U-004 is narrowed but remains unresolved: this closes one collection/item modification-time consistency case, while draft-wide sessions, locking, errors, aliases, variants, signals, and multiple-client behavior remain unestablished.
- F-040, F-045, F-046, every other finding, Opportunity, and unresolved item remain unchanged.

Non-goals:

- no clock injection, timestamp argument/public setter, monotonic counter, strict-increase guarantee, subsecond precision, future-time/created-before-modified validation, clock-source/error, F-038 limit, or timestamp field/type/unit change;
- no missing/stale item-interface error change, no-op-success change, object-server lifecycle, property-change signal, D-Bus name/path/signature/return/error, item create/replace/delete, collection label/delete, alias, session, locking, or broader Secret Service conformance change;
- no new dependency or zeroization move into the store crate; no weakening/removal of old-secret, rejected-candidate, or displaced-live-secret zeroization;
- no candidate/publisher framing/ACK/REJECT/commit behavior, host validation, resource format/size, backend/writeback, namespace/privileged chain, VM, security boundary, later finding, Opportunity, or unresolved-item implementation;
- no broad store API/refactor, public generic mutation closure, model visibility change, test deletion/repartition, generated artifact, formatting-only churn, or F-040 Rustdoc correction.

Validation commands and evidence:

- `cargo test -p underwrap-secretsd-store set_`
- `cargo test -p underwrap-secretsd dbus_service::item::tests`
- `cargo test -p underwrap-secretsd dbus_service::collection::tests::item_setters_update_collection_modified_property`
- `cargo test -p underwrap-secretsd-store`
- `cargo test -p underwrap-secretsd`
- `cargo test -p underwrap-protocol resource_blob`
- build the exact Nix `web-build` artifact through `UNDERWRAP_WEB_DIST`, then run `cargo test --workspace --exclude integration-tests -- --test-threads=4`
- `./crates/integration-tests/run`
- `cargo fmt -- --check`
- workspace Clippy with warnings denied and the exact Nix web artifact
- `nix build .#checks.x86_64-linux.workspace-test`
- direct source/test inspection that all three setters route through one private helper, each successful update samples one timestamp and assigns it to both objects, missing-item and secret-zeroization behavior remain, no extra clock read or dependency appears, and no secret value enters diagnostics/docs
- `git diff --check`

A VM run is not required because no VM-required boundary changes: NixOS module, packaging, identities, namespaces, capabilities, privileged chain, process topology, D-Bus signature, object registration, or property signal. Deployed `Collection.Modified` semantics intentionally change, and direct production store, handler publication/commit, item getter, and table-driven collection getter tests exercise that exact behavior.

- [x] Revalidate F-039 against current store/item handlers, candidate/ACK and zeroization behavior, property getters, tests, documentation, F-012/F-038 effects, U-004, and historical report/handover. Evidence: direct CodeGraph/source/test/doc inspection and two bounded design/evidence consultations on 2026-08-30; all three setters still omit the containing collection, while create/replace/delete already update it.
- [x] Obtain fresh independent approval of the private-helper/narrow-setter design, zeroization and missing-item preservation, same-sample semantics, test/property matrix, docs owner, allowlist, non-goals, cross-effects, validation, and no-VM rationale before cleanup discovery. Evidence: design review passed; evidence review required all-three `Collection.Modified` getter coverage, an exact focused command, qualification of arbitrary versus generated timestamps, and an accurate no-VM rationale that acknowledges deployed semantic change. The plan incorporated every correction and fresh re-review passed on 2026-08-30 with execution evidence pending.
- [x] Complete finding-local and repository-wide cleanup discovery; independently process accepted cleanup before timestamp propagation.
  - Finding-local review found all item getters/setters, direct SetSecret mutation/zeroization, collection getter, tests, and candidate publication paths live; no independent net deletion exists.
  - Private helper extraction, narrow SetSecret store method, duplicate manual-clock removal, test consolidation/additions, and docs are the F-039 fix rather than cleanup.
  - Repository-wide bounded private-definition/reachability review found no high-confidence dead production deletion independent of current or later findings.
  - Broad store/D-Bus refactors, tests/docs deletion, dependencies, generated files, later findings, formatting, and cosmetic churn are rejected.
  - Cleanup is exhausted with no accepted unit and no cleanup commit; the approved timestamp propagation may begin.
- [x] Implement and independently review the shared setter path, accepted/rejected handler evidence, property observation, and documentation without changing F-012 or missing-item behavior.
  - Fix commit: `71abba9b20a2c643f038c2b75e93e6c1d897d4b0` (`fix: propagate item modifications to collections`).
  - One private helper now resolves an existing item, applies one narrow setter mutation, samples `now_secs()` once, and stamps the item and containing collection with that exact value. Secret, attributes, and label setters all route through it; missing targets return `false` before clock access or mutation.
  - `Item.SetSecret` retains candidate old-secret zeroization and delegates replacement to the new narrow store setter; the existing candidate publication, exact-ACK commit, rejected-candidate cleanup, displaced-live-secret zeroization, and ignored missing-item result remain unchanged.
  - Store, handler, and property tests cover all three setters at backdated baselines; ACK publishes/commits matching metadata and both property values, REJECT publishes a synchronized candidate while preserving accepted structure, and each missing collection/item case runs independently on a fresh fixture.
  - Initial production review passed. Test/docs review required replacing an overbroad byte-level claim and isolating each missing-setter case so later calls could not mask an earlier mutation. A separate correction worker implemented both; fresh re-review passed the exact seven paths, and final post-validation review passed implementation, F-012 preservation, tests, docs, scope, and evidence with no gap.
- [x] Validate, run controller scope/diff/artifact checks, commit coherently, and record F-039 completion/cross-effects before promoting F-040.
  - Exact staged diff: seven approved paths, 413 insertions and 96 deletions, SHA-256 `97735060e62f099d7911f69b2ad6ddbe0b9a00e3a7464fdd5c0276cd14f47060`.
  - Focused/full validation passed 3 store setter tests, 8 item handler tests, 1 collection-property test, all 26 store tests, all 42 secretsd tests, and 25 focused protocol resource-blob tests (log SHA-256 `9463d97c357dae98bada32b5039cb75c1a4edf8d1b383cfe17afbdcfd1a8bcca`).
  - Bounded non-integration workspace tests passed with exact Nix web artifact `/nix/store/q6j5j105c1y9ilcppwaph4y9j36wirrz-underwrap-web-dist-0.1.0` (workspace log SHA-256 `75dbba606adbd3904eecf544cadb4ef83787a5489aaccc29ab153dcf6f08f7c3`; prerequisite log SHA-256 `33fec8fdf10471630c29c0c98e6ebbe21fce3ccb07019bab8ff0fab74378da14`).
  - The integration runner passed all 105 tests (log SHA-256 `d31152e6c85a9be18336090bcde7e89e9177d094584a885dd7a89e42bbb08930`); rustfmt and full workspace denied-warning Clippy passed (log SHA-256 `c89c5218ffefea992e7ea7dc3f812d6d8bb041eb8da1f50192fbca3bbd41d373`); the Nix workspace test built successfully (log SHA-256 `4d228b715add0bf0775f02353f830ee77980cb8ded8a4949a2661896b226ed3c`).
  - The controller's first static inventory expected four `now_secs()` call sites and stopped after all named validation had passed. Source inspection corrected the count to three—create-or-replace, delete, and the shared setter helper—and the exact rerun passed; final review classified this as an evidence-script correction only, not an implementation or test failure.
  - Controller checks passed exact seven-path scope, three setter routes/one shared clock sample, old-secret zeroization and missing-target preservation, no manual item-handler clock, no secret-bearing docs/diagnostics, no Cargo/dependency/schema/generated/security/later-finding change, `git diff --check`, artifact absence, staged checksum, and clean post-commit tree.
  - F-039 is resolved for secret, attributes, and label mutations. The two modified fields receive one sampled Unix-second value after a successful setter, but no strict numerical increase or loaded-timestamp relation is claimed.
  - F-012 and F-038 remain resolved; U-004 is narrowed but remains unresolved for the rest of Secret Service draft conformance; F-040, F-045, F-046, and every other finding/Opportunity/unresolved item remain unchanged. F-040 is next.
  - Final closure review found no semantic correction, requested controller-only Git/hash verification, and then passed after exact plan-only status, seven-path 413/96 stat, patch checksum, all six retained log hashes, and `git diff --check` were reconfirmed. F-040 promotion is approved.

### Completed iteration — F-040 secretsd-store ownership Rustdoc

F-040 remains valid with an obsolete historical remedy at clean HEAD `de7a8c14b9a0f484a7898b95bfbb1495a2d544ab`.

`underwrap-secretsd-store` crate Rustdoc still says its complete state is a JSON envelope sent only over an authenticated daemon socketpair and never written locally. The crate actually owns only the mutable in-memory `SecretStore` model and in-process operations; collection/item creation samples OS time and item ids use OS randomness, so the model is not mathematically pure.

Production `underwrap-secretsd` initializes that model by converting the canonical `LibsecretSection` received through the one-shot PID-2 init pipe. It converts each candidate back to a canonical section, publishes it through the private secretsd→host publication socket, and commits candidate memory only after exact ACK. Host/backend persistence is outside this crate.

The public, non-test-gated `from_json`, `to_json`, and `to_json_pretty` envelope helpers have no current in-repository production callers: `from_json`/`to_json` are referenced only by three sibling tests and the pretty helper has no in-repository caller. Static repository search does not establish absence of external downstream use, but this greenfield workspace has no published compatibility surface. F-008 removed the historical target-writable tmpfs state pathname, so F-040 must not restore the report's old “canonical tmpfs flow” recommendation.

Consequence: maintainers are currently told the wrong component ownership, transport, serialization, and persistence invariants for secret-bearing state.

Security boundary: none. Cleanup 1 separately removed serialization surface with no current in-repository production callers; the remaining iteration changes documentation only. F-008/F-012 transport, validation, ACK, zeroization, and persistence boundaries remain unchanged.

Current ownership flow:

- `underwrap-secretsd-store`: in-memory collections/items/aliases and in-process model operations; no file, socket, publication, canonical-resource, or backend I/O.
- `underwrap-secretsd`: one-shot canonical-section initialization, store↔section conversion, candidate canonicalization/publication, exact-ACK commit, and in-process zeroization policy.
- host `underwrap-ns`: private publication receiver, validation/retention/ACK, final resource composition, and Checkin dispatch through `underwrapd`.
- `underwrapd`: scoped resource-channel forwarding of runtime finalize.
- backend: canonical complete-resource persistence through `StorageBackend`.

Exact documentation-fix scope on the resulting tree:

- Rewrite `crates/underwrap-secretsd-store/src/lib.rs` crate Rustdoc to describe only the in-memory state model and its ownership boundary, while accurately identifying the separate `underwrap-secretsd` conversion/publication integration without claiming this crate performs I/O, persistence, canonicalization, ACK handling, or zeroization.
- Correct the crate package description in `Cargo.toml` so it no longer presents removed JSON-envelope serialization as the crate's defining production role.
- State that the in-memory model contains raw secret bytes and that external callers own lifecycle, transport, persistence, and memory-erasure policy; do not quote or add secret values.
- State that production `underwrap-secretsd` owns store↔canonical-section conversion and publication integration; cleanup 1 removed the generic JSON helpers, so no JSON method documentation remains.
- Record only static call-path and package-Rustdoc validation evidence in `docs/testing.md`; keep mechanism ownership normative in crate Rustdoc plus `docs/architecture.md` and `docs/secretsd-security.md` rather than duplicating the full flow.

Resulting documentation-fix allowlist:

- `crates/underwrap-secretsd-store/Cargo.toml` — package description only
- `crates/underwrap-secretsd-store/src/lib.rs`
- `docs/testing.md`
- this plan for evidence

Approved cleanup 1 — remove the unused public JSON serialization surface:

- Delete `SecretStore::{from_json, to_json, to_json_pretty}` and `src/store/json.rs`, and remove `mod json`.
- Remove `Serialize`/`Deserialize` derives/imports and item Serde attributes from the in-memory model, plus `default_content_type` and `base64_bytes`, because static repository search finds no remaining consumer after helper removal.
- Remove direct crate dependencies `serde`, `serde_json`, and `base64`; retain `anyhow` and `getrandom`. Refresh only `Cargo.lock`'s `underwrap-secretsd-store` dependency list; package records remain because other workspace crates use them.
- Remove exactly `json_roundtrip_empty`, `json_roundtrip_with_items`, and `binary_secret_roundtrip`, which exist only for the deleted JSON API.
- Preserve production evidence in `underwrap-secretsd::section_store` tests: store↔canonical-section round trip, arbitrary/non-UTF-8 secret bytes, metadata, canonical generation/parsing/idempotence, and explicit section-to-store secret preservation.
- External downstream use of the public methods cannot be disproven by repository search; API removal is accepted under the greenfield/no-published-compatibility posture.
- Cleanup allowlist: `Cargo.lock`, `crates/underwrap-secretsd-store/Cargo.toml`, and `src/store/{helpers,json,mod,model,tests}.rs` within that crate; no root manifest, flake, `underwrap-secretsd`, documentation, or package-description change.
- Cleanup validation: refresh then require the lock, exact symbol/dependency/cargo-tree absence, focused/full store and secretsd section-conversion tests, strict package Rustdoc, denied-warning Clippy, rustfmt, bounded workspace tests, integration runner, Nix workspace test, artifact/scope/secret checks, and `git diff --check`.

Expected cross-effects:

- F-008 and F-012 remain resolved; no pathname, publication, ACK, candidate, zeroization, or persistence behavior changes.
- F-041's broader persistent-secret-store claim and F-042's daemon opaque-blob parsing claim remain unresolved and unchanged.
- F-045, every other finding, Opportunity, and unresolved item remain unchanged.

Non-goals:

- no target-writable tmpfs state path, daemon socket-only flow, new JSON/canonical format, serialization/persistence/transport behavior, file/socket/backend I/O, candidate/ACK/zeroization, resource format/size, or Secret Service behavior change;
- no product claim that frontend memory, target behavior, retained backend generations, or external configured targets cannot persist or disclose their own secret;
- no F-041 persistent-store-claim rewrite, F-042 daemon parser documentation, F-045 alias handling, public architecture/security procedure duplication, later finding, Opportunity, or unresolved-item implementation;
- no dependency addition, generated artifact, NixOS/module, VM, privileged chain, security boundary, broad documentation rewrite, historical record edit, test deletion, or code removal in the documentation fix; only a separately scoped, independently accepted cleanup may remove the helper surface and tests/dependencies that exist solely for it.

Validation commands and evidence:

- direct source/call-path search for every `SecretStore` JSON helper and each actual store↔section/init/publication/persistence owner
- `cargo test -p underwrap-secretsd-store`
- `cargo test -p underwrap-secretsd section_store`
- `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps -p underwrap-secretsd-store`
- `cargo clippy --all-targets -p underwrap-secretsd-store -p underwrap-secretsd -- -D warnings`
- `cargo fmt -- --check`
- build the exact Nix `web-build` artifact through `UNDERWRAP_WEB_DIST`, then run bounded non-integration workspace tests and workspace denied-warning Clippy
- `./crates/integration-tests/run`
- `nix build .#checks.x86_64-linux.workspace-test`
- direct scope/docs inspection that no obsolete tmpfs/socket-only flow, local persistence claim, secret value, F-041/F-042 wording, dependency/generated artifact, or unrelated behavior change enters the unit
- `git diff --check`

A VM run is not required because no NixOS, packaging, identity, namespace, capability, privileged-chain, process-topology, target-visible protocol, or deployment behavior changes; package Rustdoc, direct call-path evidence, crate/workspace tests, and the integration runner cover this maintenance boundary.

- [x] Revalidate F-040 against current crate/module docs, JSON-helper reachability, one-shot initialization, section conversion, publication/ACK, host/backend persistence, F-008 effects, current public documentation, and historical report/handover. Evidence: direct CodeGraph/source/test/doc inspection on 2026-08-30; the false crate invariant persists, all JSON helper callers are test-only or absent, and the historical tmpfs remedy is superseded.
- [x] Obtain fresh independent approval of the ownership wording, conditional JSON-helper treatment, docs owners, allowlist, non-goals, cross-effects, validation, and no-VM rationale before cleanup discovery. Evidence: initial reviews required in-process rather than pure wording, public/no-current-production reachability precision, a secret-bearing JSON/base64 caveat if retained, the `underwrapd` finalize hop, evidence-only testing docs, package-description correction, and explicit cleanup-versus-fix ownership. After those corrections and removal of the unsupported word “unreachable,” fresh re-review passed on 2026-08-30 with external downstream use explicitly unknown.
- [x] Complete finding-local and repository-wide cleanup discovery; independently process accepted cleanup before the Rustdoc correction, then revise the fix allowlist to the resulting tree.
  - Finding-local discovery accepted cleanup 1 exactly as recorded above; two-stage independent review verified repository reachability, production replacement coverage, dependency/lock impact, greenfield API posture, and no protected-boundary change.
  - Repository-wide bounded discovery found no other high-confidence private production deletion independent of current or later findings.
  - Crate/package/testing wording is the fix; broad store/section/publication refactors, current production tests, architecture/security duplication, later findings, formatting, and cosmetic churn are rejected.
  - Cleanup is exhausted after cleanup 1.
- [x] Implement, independently review, validate, and commit cleanup 1 before the Rustdoc correction.
  - Commit: `e9c8f298e9ba2744659b37111da2dbf28bd66173` (`refactor: remove unused SecretStore JSON surface`).
  - Exact diff: seven approved paths, four insertions and 124 deletions, SHA-256 `1d88a9ed158b68325df2ac01cc4a595e5e3f2ef67471d1faa4ba47f440b50d2a`; only the three public JSON helpers/module, Serde/base64 derives/attributes/helpers, three helper-only tests, and direct crate dependency/lock edges were removed.
  - The crate's locked normal/feature trees now contain only `anyhow` and `getrandom`; shared Serde/base64 package records remain for other crates. Package description, crate Rustdoc, root manifest, flake, `underwrap-secretsd`, and documentation were untouched.
  - Validation passed 23 store tests, six focused section-conversion tests, 42 full secretsd tests, strict denied-warning package Rustdoc, focused denied-warning Clippy, and rustfmt (log SHA-256 `dfa509d46af8ec0d6fd3a78b98770cfc1def27e7cdcc9edc57f5342fdcaffb45`).
  - Bounded workspace tests passed with exact web artifact `/nix/store/q6j5j105c1y9ilcppwaph4y9j36wirrz-underwrap-web-dist-0.1.0` (workspace log SHA-256 `d8ddb1e2bf5cd8f5599e6685e37f260438dc730b71de507f2a6e4ecaede2c3fe`; prerequisite log SHA-256 `33fec8fdf10471630c29c0c98e6ebbe21fce3ccb07019bab8ff0fab74378da14`); the integration runner passed 105 tests (`0f4520dac6480e342fef9cedb4c78746189b3f02ef70f0b57a467db73a0df886`), workspace Clippy passed (`6a924049ad5ae3bc7a66ad12699f624f351beb32a3f26de876294cc2042635e2`), and Nix workspace-test passed (`3581062811b6935948adc748ce2b14938d10b524a4ab9e908506d74e71f1a32c`).
  - Initial post-implementation review confirmed semantics but withheld on unavailable diff/log evidence; after controller exact scope/stat/hash and full validation, fresh re-review passed with no gap.
  - Controller checks passed exact symbol/dependency/cargo-tree absence, production section-conversion replacement coverage, no package-description/doc/secret/security/later change, lock scope, `git diff --check`, checksum, artifacts, and clean post-commit tree. Unknown external downstream use remains explicitly accepted under greenfield posture.
- [x] Implement and independently review the resulting-tree ownership Rustdoc/package/testing evidence without changing secret-state behavior or adjacent claims.
  - Fix commit: `393c3a3ee108b198a7b509b5e1a602273b61d94d` (`docs: correct secretsd-store ownership`).
  - Package metadata and crate Rustdoc now identify one in-memory state model containing raw secret bytes and in-process time/randomness operations, explicitly disclaim crate-level file/socket/canonicalization/publication/ACK/backend/persistence I/O and memory-erasure guarantees, and assign lifecycle/transport/persistence/erasure policy plus store↔section publication integration to callers.
  - `docs/testing.md` records strict package Rustdoc and static dependency/call-path commands only, links the normative architecture/security owners, and disclaims any proof that callers, configured targets, or later persistence domains cannot retain/disclose data.
  - Initial implementation review required replacing the inaccurate claim that raw bytes remain for the whole store lifetime; a separate correction worker stated raw-byte representation with no crate erasure guarantee. Fresh post-correction and final post-validation reviews passed the exact three paths, ownership chain, nonclaims, and F-041/F-042 exclusions with no gap.
- [x] Validate, run controller scope/diff/artifact checks, commit coherently, and record F-040 completion/cross-effects before promoting F-041.
  - Exact fix diff: three approved paths, 31 insertions and 12 deletions, SHA-256 `3a6d3a16613d7b0d8c699b0eacd23248228d704abef231e849464af500db018b`.
  - Focused validation passed 23 package tests, six section-conversion tests, strict denied-warning package Rustdoc, focused denied-warning Clippy, rustfmt, dependency tree, and ownership call-path inventories (log SHA-256 `11c6afaac83565f1faab4440827b85d7e56431009b6e7cab8fa9436e2c18d1c9`).
  - Bounded workspace tests passed with exact Nix web artifact `/nix/store/q6j5j105c1y9ilcppwaph4y9j36wirrz-underwrap-web-dist-0.1.0` (workspace log SHA-256 `dac197ed0dd59686e3b0675cce385df9c6e65df0919c53eadf297b1092cb3302`; prerequisite log SHA-256 `33fec8fdf10471630c29c0c98e6ebbe21fce3ccb07019bab8ff0fab74378da14`); integration passed 105 tests (`d79d5014f79e1d622988e6a714402ad35d15280560540536795ebab30daed45d`), workspace Clippy passed (`704b10129edcca4163fd389b9ce8fef2c848be0e7ea1c09c7675d6e0b1c75631`), and Nix workspace-test passed (`9cf5b3a8bf57c00ef6477c253a417eeb36eddf5f3f38aa80b005d4f7f89b6151`).
  - Controller checks passed exact package-description-only manifest change, no lock/store/source behavior change, obsolete tmpfs/socket-only/JSON-envelope wording absence, no secret value or F-041/F-042 claim, package/docs scope, `git diff --check`, checksum, artifacts, and clean post-commit tree.
  - F-040 is resolved on the resulting tree: no generic JSON helper remains, and the crate/package documentation describes current ownership without restoring the superseded tmpfs path.
  - F-008 and F-012 remain resolved; F-041, F-042, F-045, every other finding, Opportunity, and unresolved item remain unchanged. F-041 is next.
  - Final closure review passed the ordered cleanup/fix, ownership wording, external-use caveat, validation, nonclaims, cross-effects, no-VM rationale, and F-041 promotion. The controller then independently reconfirmed plan-only status, both commit stats/patch checksums, all twelve retained log hashes, and `git diff --check`.

### Completed iteration — F-041 scoped sensitive-persistence claims

F-041 remains directly reproducible at clean HEAD `b7bc5c0adcb04320d70851ed0a720cbe08edefa5`.

`docs/security.md` says the backend is the only persistent secret store and all durable secret material passes through `StorageBackend`; `AGENTS.md` repeats the universal rule, and task-oriented docs repeat broad “persistent secret material/storage” wording.

The implementation intentionally persists sensitive non-resource state elsewhere:

- one frontend runtime client instance identity PEM containing its mTLS private key;
- the operator-provisioned backend HTTPS server private key;
- per-user Argon2id password-verifier records;
- trusted frontend/backend configuration and paths, including fixed `envSet` values generated into Nix daemon configuration.

Encrypted-store master keys are generated and held in backend memory, displayed once, and never written by underwrap; external operator custody is separate.
A configured target is trusted with its projection and may persist or disclose it outside underwrap's writeback path.

Consequence: the universal wording hides distinct credential/configuration boundaries and can cause maintainers to remove legitimate credential installation or overstate what backend writeback policy controls.

Security boundary:

- Protected assets: brokered resource values, the frontend runtime mTLS private identity, backend TLS private key, CA signing key, admin password verifiers, and trusted configuration integrity.
- Attackers: a local non-root reader of credential/resource files, an offline reader of copied records/ciphertext, or an unauthorized writer to trusted configuration; host root, backend host root, and the backend remain trusted by the project threat model.
- Plaintext resource direction: current plaintext material read → current brokered-resource disclosure.
- Encrypted resource direction: active or retained ciphertext plus its corresponding generation key → current or stale brokered-resource disclosure.
- Runtime identity direction: frontend identity read → runtime-client impersonation.
- Backend TLS direction: server-key read → backend/server impersonation; CA signing-key read → unauthorized issuance of certificates trusted by its consumers.
- Password direction: verifier-record read → offline guessing → possible admin credential compromise.
- Configuration direction: trusted-config write → target/backend behavior manipulation; trusted-config read → disclosure only, with Nix-generated frontend configuration explicitly non-confidential.
- Configured target: trusted recipient and excluded persistence destination, not an attacker in this invariant.
- Change direction: documentation narrows which asset the `StorageBackend` invariant protects and enumerates existing controls/nonclaims.
  No filesystem permission, authentication, persistence, or authorization mechanism weakens.

Exact scope:

- Rename security invariant 1 around **underwrap-managed durable persistence of brokered resource material**.
  State that active resource values use the backend's selected `StorageBackend`.
- Describe retained encrypted `backup/` and `stage/` generations as ciphertext produced by encrypted backend storage but retained in sibling recovery workspaces outside the active canonical `StorageBackend` path and APIs.
  State that retention is indefinite, decryptable with the corresponding generation key, unaffected by resource/store deletion, and owned by trusted-host cleanup.
- Keep frontend brokered resource state in process memory, except for files bytes staged on invocation-private `noswap` tmpfs.
  Describe libsecret state as in-memory and its namespace-private bus tmpfs as runtime socket/helper artifacts without a `noswap` claim.
  Continue prohibiting ordinary temp files, sidecars, local databases, disk caches, or alternate durable frontend paths for brokered resource values.
- Explicitly state that this scoped invariant is not a guarantee about independently managed credentials/authentication records, trusted configuration, operator-custodied keys, or persistence/disclosure by the configured target.
- Enumerate the normal NixOS runtime-identity controls exactly: a fixed path under a mode-`0700` per-instance state directory, config-derived daemon ownership, mode-`0400` synced same-directory atomic replacement, and read-only daemon service access.
  Preserve keygen's owner precedence outside that normal case: configured daemon account, existing identity owner, then invoking identity.
  State that daemon startup rejects a present identity with wrong owner or file mode wider than `0600`, and checks parent owner/mode `0700` or tighter only when parent metadata lookup succeeds.
  Retain the no-parent-fsync, no-secure-erasure, no-install/enrollment transaction, no-stdout-delivery transaction, and no-live-reload limits.
- Enumerate the backend HTTPS private key and any CA signing key as operator-managed credential files outside `StorageBackend`.
  State that the backend reads its configured server key into rustls but does not install, rotate, atomically replace, or validate its ownership/mode.
  Treat backend-host service identity, filesystem ownership/mode, restrictive umask, sandboxing, and CA-key removal/custody as deployment/operator controls.
- Enumerate admin password hashes as sensitive offline-verifier records under `users_dir`, outside `StorageBackend`.
  State that records use Argon2id, are omitted from API responses, redact hashes from `Debug`, and use `NamedTempFile` restrictive Unix temporary-file creation plus file sync and atomic rename.
  State that the application neither chmods/verifies the final record mode nor enforces the directory mode; deployment ownership/mode/umask remain controlling.
  Retain the no-parent-fsync, no encryption-at-rest, and no secure-erasure claims.
- Clarify that Nix-generated frontend configuration is trusted integrity state, not a confidential secret store.
  State that generated Nix configuration can be readable outside the daemon and read-only service access protects integrity rather than confidentiality.
  `envSet` values must be non-secret fixed values; operators put brokered values in resource slots instead.
  Backend YAML contains paths/settings, including a TLS-key path, rather than TLS key bytes or encrypted-store master keys.
- State the encrypted-store master-key contract exactly: generation in backend memory, one-time create/rotate response, no underwrap-written key file, and `MasterKey` zeroization on final drop.
  State that unlocked-backend memory compromise can recover the installed key and that request/response/browser/clipboard copies are outside comprehensive erasure.
  Do not claim response delivery, external custody, TPM/HSM, or external-vault guarantees.
- Preserve the target limitation: backend read-only/writeback policy does not prevent a trusted target from printing, storing, or transmitting its projection elsewhere.
- Apply concise lexical scoping to `docs/architecture.md`, `docs/agent-facing-wrappers.md`, and `docs/remote-backend.md` without duplicating the normative security inventory.
- In `docs/remote-backend.md` and `docs/configuration.md`, identify `https.tls.key_file` and CA signing keys as operator-managed persistent authentication material outside resources/`StorageBackend`.
  Do not invent an application-enforced mode.
- In `docs/configuration.md`, warn that `envSet` enters Nix-generated configuration and is not secret-bearing input.
- In `AGENTS.md`, scope the engineering rule to brokered resource material, preserve the frontend cache prohibition, enumerate separate credential/auth/config domains, and prohibit secrets in `envSet`.
  Synchronize the `underwrap-secretsd` key-file description from a tmpfs state model to one-shot canonical initialization, in-memory state, and private generation publication, recording that F-040-adjacent guidance cross-effect explicitly.
- Record only static claim/control evidence in `docs/testing.md`, including the existing keygen/daemon/user/Nix witnesses and the limits of Nix evaluation.
  Do not duplicate normative security prose.

Fix allowlist — seven product/guidance files:

- `AGENTS.md`
- `docs/security.md`
- `docs/architecture.md`
- `docs/agent-facing-wrappers.md`
- `docs/remote-backend.md`
- `docs/configuration.md`
- `docs/testing.md`

The active plan changes separately for workflow evidence only.

Expected cross-effects:

- F-041 resolves universal persistence-claim precision; implementation controls remain unchanged.
- F-040 remains resolved, and its current mechanism owner is synchronized by correcting the stale `AGENTS.md` key-file description.
- F-008, F-012, F-034, and the resource-persistence rules remain resolved and unchanged.
- F-042's daemon opaque-blob parsing claim, F-050's stale Playwright key fixture, F-051's unrelated stale/temporal prose, and every other finding/Opportunity/unresolved item remain unchanged.

Non-goals:

- no resource persistence, encrypted rotation generation, master-key custody, credential generation/install/reload, TLS loading, user-record persistence, password hashing, Nix configuration generation, env projection, target/writeback behavior, permission mode, or authorization change;
- no new/chosen exact backend TLS-key or users-directory mode, backend service module, automatic chmod/chown, key rotation, secret manager, TPM/HSM, encrypted auth registry, session persistence, external-vault integration, or secure-erasure/power-loss guarantee;
- no claim that public certificates/fingerprints are secret, that configuration read-only access supplies confidentiality, that targets cannot persist data, or that backend read-only permission creates a behavioral sandbox/read-only mount;
- no F-042 daemon parser wording, F-050 Playwright fixture, F-051 broad documentation cleanup, historical record edit, Nix/Rust/SPA executable change, dependency/generated artifact, VM change, security-boundary weakening, later finding, Opportunity, or unresolved-item implementation.

Validation commands and evidence:

- direct claim/source matrix covering `StorageBackend`, encrypted generations, runtime identity path/install/admission/service access, backend TLS loading, user hash persistence/redaction/API omission, Nix config materialization, envSet, master-key handling, and target limitations
- `cargo test -p underwrap-keygen`
- `cargo test -p underwrapd startup::tests::credential_permissions`
- `cargo test -p underwrap-backend users`
- `nix build .#checks.x86_64-linux.frontend-security-assertions-eval`
- `nix build .#checks.x86_64-linux.examples-nixos-eval`
- `nix flake check --all-systems --no-build`
- direct Markdown link/anchor and present-tense, one-sentence-per-line review across all seven fix files
- repository-wide search for unscoped “only persistent secret store”, “persistent secret material/storage”, and secret-bearing `envSet` guidance
- exact diff/scope check proving only documentation, guidance, and Nix-option references are described, with no Nix module source change
- `git diff --check`

A VM run is not required because the final allowlist changes no Nix/Rust executable behavior, module options, service settings, filesystem modes, identities, namespaces, capabilities, privileged chain, or runtime protocol. Existing source/unit/VM evidence is cited as prior control evidence; fresh Nix evaluation proves only that unchanged module/examples still evaluate, not deployed ownership/mode enforcement.

- [x] Revalidate F-041 against current universal claims, backend resource/policy storage, retained encrypted generations, runtime/backend TLS keys, user records, configuration/envSet, master-key custody, target behavior, keygen/daemon/Nix controls, tests, and historical report/handover. Evidence: direct CodeGraph/source/docs/test inspection and two focused design/evidence reviews on 2026-08-30; the broad claim persists and the separate sensitive domains/limits above are current.
- [x] Obtain fresh independent approval of the scoped invariant, complete asset/control/nonclaim inventory, threat-boundary wording, task-document edits, allowlist, non-goals, cross-effects, static validation, and no-VM rationale before cleanup discovery. Evidence: initial security/evidence reviews required per-asset read/write directions, conditional runtime-identity ownership/parent checks, exact password-record mode limits, complete master-key and retained-generation handling, CA-key custody, explicit F-040 guidance synchronization, seven-file/plan separation, and one-sentence prose. After correcting plaintext versus generation-key ciphertext disclosure and CA/server-key outcomes, fresh final re-review passed on 2026-08-30 with implementation evidence pending.
- [x] Complete finding-local and repository-wide cleanup discovery; independently process accepted cleanup before claim correction.
  - Finding-local review found no independent deletion: repeated broad claims must be lexically scoped in place, while the normative security inventory and task-specific links are required fix evidence.
  - Repository-wide bounded private-definition/reachability review found no high-confidence dead production deletion independent of current or later findings.
  - Broad documentation consolidation, removal of credential/control details, F-042/F-050/F-051 edits, tests/evidence deletion, dependencies, executable changes, formatting, and cosmetic churn are rejected.
  - Cleanup is exhausted with no accepted unit and no cleanup commit; the approved claim correction may begin.
- [x] Implement and independently review the scoped persistence claims without changing any control or absorbing F-042/F-050/F-051.
  - Fix commit: `3f2da79bf9f438e20df08ef50334c3dae8946ed2` (`docs: scope brokered resource persistence`).
  - The normative invariant now covers only underwrap-managed durable brokered resource material. Active resource values use `StorageBackend`; retained encrypted workspaces, runtime identities, server/CA keys, password verifiers, trusted configuration, encrypted master-key custody, and target-controlled persistence each have a separately evidenced domain and nonclaim.
  - Task docs now use brokered-resource vocabulary, identify TLS/CA custody and non-enforcement, prohibit secrets in Nix-materialized `envSet`, and preserve the trusted-target persistence limitation. `AGENTS.md` mirrors the engineering rule and synchronizes F-040's secretsd entry to one-shot canonical initialization, in-memory state, and private generation publication.
  - The implementation worker exhausted its runtime after completing the seven-file draft. Initial security/scope reviews found an incorrect `noswap` claim for libsecret bus tmpfs, incomplete threat directions, and imprecise secretsd wording. A separate correction worker fixed them; a second review required the encrypted ciphertext+generation-key direction and one heading anchor, and a second correction pass fixed both. Final content and evidence re-reviews passed with no gap.
- [x] Validate, run controller scope/diff/artifact checks, commit coherently, and record F-041 completion/cross-effects before promoting F-042.
  - Exact diff: seven approved product/guidance files, 172 insertions and 114 deletions, SHA-256 `918ee6517f9500a315a74f356030c21d87d7d45e9a29810845755d9823e9b42f`.
  - Focused controls passed 27 keygen tests, four daemon credential-permission tests, and 28 backend user tests with exact Nix web artifact `/nix/store/q6j5j105c1y9ilcppwaph4y9j36wirrz-underwrap-web-dist-0.1.0` (focused log SHA-256 `656735059ea90cda6161185c009d590750adf6a499487a1febb4de8e0a8eee26`; prerequisite log SHA-256 `33fec8fdf10471630c29c0c98e6ebbe21fce3ccb07019bab8ff0fab74378da14`).
  - `frontend-security-assertions-eval` and `examples-nixos-eval` each completed with explicit PASS markers (SHA-256 `d67255cc0d73a4c265c1b123753837a4530cbfecb877a8b1567615ac7546e633` and `4eeaed70a5c09da580bc1594d17259cbd61a67567e73d5b66b261ec0fa5d4e69`); all-system flake evaluation passed (`99c9b8ff69df467630a42b55c953c9cac6bd6480ee03bc7a0023df06de8274d3`).
  - Local Markdown targets/anchors passed across all seven files (`1de4649bfb111bd963497b38d0281250d4938ab8b9c757c1881e136893637599`); exact scope/no-Nix-code diff and obvious one-sentence-per-line scans passed (`d6093d79b5d22517a7104824e3953d4eb8ffaca2b0bffeb813158254c681d531`).
  - Controller checks passed the complete claim/source matrix, files-only `noswap` precision, corresponding-generation key wording, sensitive-asset/direction coverage, product-wide broad-claim removal, no secret value, no Nix/Rust/SPA/dependency/generated change, F-042 opaque line and F-050/F-051 scope preservation, `git diff --check`, checksum, artifacts, and clean post-commit tree.
  - Nix evaluation is classified only as unchanged module/example evaluation evidence; deployed owner/mode/umask behavior remains supported by existing source, focused tests, and prior VM evidence rather than a fresh VM claim.
  - F-041 is resolved without weakening a boundary. F-040 remains resolved with synchronized agent guidance; F-008, F-012, and F-034 remain resolved; F-042, F-050, F-051, and every other finding/Opportunity/unresolved item remain unchanged. F-042 is next.
  - Final closure review initially misclassified plan-only commits `0ab2e01`, `b92ca09`, and `bdbc2f5` as product drafts. Exact Git evidence showed each changed only this plan and that `3f2da79` owns all seven product/guidance paths plus both uncommitted correction passes; fresh re-review then passed chronology, exact 172/114 stat/checksum, plan-only status, closure, and F-042 promotion.

### Completed iteration — F-042 daemon resource-blob parser boundary

F-042 remains directly reproducible at clean HEAD `4f469ae77a15e0f6cad6b4fd6302df3afc2f055d` with stale historical wording after F-009 and F-038.

`docs/security.md` still calls the daemon's cached resource blob opaque canonical bytes.
`underwrapd::wrapper_flow::read` actually deserializes the complete secret-bearing `ResourceBlob` before caching the original decoded bytes.

After F-009, env values are not copied into `ReadOk` or fd 3.
The daemon conditionally inspects only env names, but typed deserialization still materializes env, files, and libsecret maps, metadata, and secret values in temporary daemon memory.

Consequence: security review and zeroization/copy inventories omit one parser/allocation site, while files/libsecret semantic opacity is confused with byte-level opacity.
No current authorization bypass is alleged because the backend is trusted and remains policy authority.

Security boundary:

- Protected asset: confidentiality and availability/integrity of brokered resource values while a daemon opens and retains one authorized resource.
- Failure source: malformed or unexpected trusted-backend output, storage/backend bugs, parser/dependency defects, or a maintenance change to schema/decoding/copy/cache behavior.
  A local caller can trigger an authorized open but cannot directly choose backend blob bytes.
- Boundary direction: authenticated backend HTTP JSON/base64 → ordinary response `String` → decoded resource-byte `Vec` → complete typed secret-bearing `ResourceBlob` parse → conditional env-name validation → original decoded bytes cached for authenticated fd-20 Checkout.
- Change direction: documentation and mandatory review scope become accurate.
  No parser, authorization, transport, memory-lifetime, or confidentiality mechanism changes.

Exact behavior to document:

- For successful responses, reqwest first buffers the complete response body in ordinary non-zeroizing storage and then Serde allocates the owned open-response DTO strings, including `blob_b64`.
  Neither layer has a daemon application-level response-body cap, and the existing admitted-call deadline covers buffering plus deserialization.
  Non-success responses are likewise read through uncapped `response.text()`, but that path does not parse a resource blob.
- `read.rs` standard-base64-decodes `blob_b64` into `blob_bytes` before the decoded complete-resource size check.
- A successful `ResourceBlob::from_canonical_bytes` call first enforces the inclusive 2,097,152-byte decoded resource limit.
  Serde then parses the complete typed JSON, including per-entry `SecretBytes` base64 decoding and F-038 safe-integer checks during traversal for every present collection/item timestamp.
  Only after the complete typed parse does the method require `schema_version == 1`.
- Typed parsing materializes owned keys/metadata and decoded secret-bearing `SecretBytes` for every env, files, and libsecret entry even when the wrapper projects no env slot.
  The temporary parsed `SecretBytes` vectors zeroize on drop.
- When env projection is enabled, the daemon additionally validates every env name against the reserved/hardening grammar.
  When env projection is disabled, it does not validate env names.
- Unknown fields are ignored by the outer open-response DTO, `ResourceBlob`, and nested structs.
  Known duplicate struct fields reject through derived Serde visitors, while duplicate keys in map-valued JSON objects are not independently rejected and later entries replace earlier ones.
  Missing sections with `serde(default)` become empty.
- The daemon does not independently establish exact JCS byte canonicality.
  Accepted noncanonical forms, ignored fields, defaulted omissions, and duplicate map keys remain in the original cached bytes even though the typed parse observes a normalized representation.
  The normal trusted backend reparses storage and JCS-reserializes before open, but that is not a daemon-side canonicality guarantee.
- The daemon does not validate files traversal/routing/overlap/projectability.
  Beyond the `2^53 - 1` bound on all four timestamp fields, it does not validate libsecret temporal relationships, future-time plausibility, identifier/path shapes, alias references, or publication composition.
  Daemon open enforces only the inclusive 2-MiB complete-resource limit and does not enforce the independent inclusive 1-MiB canonical libsecret publication-body limit.
  Namespace code later canonicalizes the embedded section, and secretsd enforces the publication limit while constructing each generation frame.
  A section can therefore pass daemon parsing but fail generation-zero publication and readiness when its canonical publication body exceeds 1 MiB.
  Those format/semantic authorities remain at the backend, namespace planning, secretsd publication, and host-validation boundaries already documented.
- The temporary typed `ResourceBlob` is dropped after validation.
  The original decoded `blob_bytes` moves into `ChannelContext.cached_blob` and remains an ordinary non-zeroizing `Vec<u8>` for the scoped channel lifetime.
- Checkout first copies `cached_blob` into `CheckoutOk.blob`, and wire encoding copies it again into the complete payload vector.
  The receiver bounds the outer frame at the resource limit plus six envelope bytes before frame allocation, then checks the inner 2-MiB length before copying the inner blob into the decoded response.
  On the sender, `cached_blob.to_vec()` occurs before wire encoding rechecks the already trusted/capped cache length.
  None of these ordinary vectors are zeroizing.
- Outer HTTP/Serde/base64 strings/buffers, the decoded cache, Checkout/frame vectors, receiver buffers, metadata allocations, allocator remnants, dependency-internal allocations, and OS/socket copies do not gain a comprehensive zeroization claim.
- The daemon serves the cached decoded bytes verbatim and the backend-supplied writable hint on Checkout.
  At Checkin, the backend rechecks policy and writability, base64 decoding, decoded size, typed/schema/timestamp parsing, exact JCS bytes, and env immutability.
  It does not add files routing/projectability or strict libsecret publication semantics at finalize.

Exact documentation/guidance scope:

- In `docs/security.md`, correct both current opaque-blob claims and make the parser/copy/cache boundary normative without duplicating full schema definitions.
- In `docs/architecture.md`, update the open-invocation flow to include backend-response decode, complete typed parse, conditional env-name validation, typed-value drop, and original-byte channel cache.
- In `docs/testing.md`, record protocol parser vectors, valid daemon/integration evidence, static parser/copy review commands, and the absence of a dedicated malformed backend-open daemon matrix or fuzz harness/execution evidence.
- In `AGENTS.md`, add `wrapper_flow/read.rs` as the secret-bearing backend-response parser/cache site and require security review when resource schema, open-response decoding, env validation, logging, cloning, cache lifetime, Checkout copies, or buffer ownership changes.

Fix allowlist — four product/guidance files:

- `AGENTS.md`
- `docs/security.md`
- `docs/architecture.md`
- `docs/testing.md`

The active plan changes separately for workflow evidence only.

Expected cross-effects:

- F-009 remains resolved: the 2-MiB decoded-resource size-before-parse boundary remains and env values remain absent from `ReadOk`/fd 3.
- F-038 remains resolved: unsafe timestamps already reject during this daemon parse and are now documented as such.
- U-009 is narrowed but remains unresolved because `SecretBytes` zeroization does not cover HTTP/base64 strings, cached bytes, Checkout/frame buffers, metadata, allocator remnants, or dependency copies.
- F-043's no-op scope/helper fields, F-054's log-hygiene evidence, and every other finding/Opportunity/unresolved item remain unchanged.

Non-goals:

- no HTTP response cap, base64 preflight, parser/canonicality/schema/default/unknown-field behavior, exact-JCS comparison, env-name rule, files/libsecret semantic validation, error mapping, logging, cache representation, Checkout copy, buffer type/lifetime, or zeroization change;
- no daemon malformed-response unit matrix, property test, fuzzer/fuzz target/corpus/execution, dependency, protocol/resource schema, backend/runtime API, namespace, secretsd, fd/wire, policy/writeback, Nix/VM, security-boundary, or deployed behavior change;
- no F-043 code cleanup, F-049 error-chain response work, F-052 comment/vocabulary correction, F-054 log-hygiene expansion, U-009 resolution, historical record edit, broad docs cleanup, later finding, Opportunity, or unresolved-item implementation.

Validation commands and evidence:

- `cargo test -p underwrap-protocol resource_blob::tests`
- `cargo test -p underwrapd`
- `./crates/integration-tests/run`
- direct call-path inventory from `BackendClient::open_request` through `open_invocation`, `ResourceBlob::from_canonical_bytes`, `SecretBytes`, `ChannelContext.cached_blob`, and Checkout frame encoding
- static search for daemon-open malformed-response tests and `cargo-fuzz`, libFuzzer, proptest, quickcheck, fuzz directories/corpora, with absence recorded rather than converted into coverage
- targeted search proving the two daemon opaque-byte claims are removed while storage-key/session-cookie/file-string uses of “opaque” remain untouched
- direct Markdown link/anchor, present-tense, and one-sentence-per-line review across all four fix files
- exact diff/scope/secret check with no Rust/Nix/dependency/generated change
- `git diff --check`

A VM run is not required because the final allowlist changes no parser, wire contract, NixOS/module, service, identity, namespace, capability, privileged chain, target-visible behavior, or deployment state.
A fresh VM would repeat a valid flow and would not prove malformed daemon-open handling, fuzz completeness, or copy/zeroization exhaustiveness.

- [x] Revalidate F-042 against current daemon open/read/channel code, ResourceBlob/SecretBytes behavior, F-009/F-038 effects, tests/fuzz inventory, current docs, and historical report/handover. Evidence: direct CodeGraph/source/test/doc inspection plus two focused design/evidence reviews on 2026-08-30; the complete typed parser/copy/cache site persists, historical env-value extraction is stale, and no dedicated malformed-open or fuzz evidence exists.
- [x] Obtain fresh independent approval of the exact parser/copy/validation/nonvalidation inventory, security boundary, four-file wording, evidence limits, allowlist, non-goals, cross-effects, validation, and no-VM rationale before cleanup discovery. Evidence: evidence-scope review passed; technical review required exact HTTP buffering, parse order, unknown/default/duplicate handling, libsecret temporal qualification, Checkout/frame copies and caps, finalize checks, and the later independent 1-MiB publication limit/readiness consequence. The plan incorporated every correction and fresh final re-review passed on 2026-08-30 with implementation evidence pending.
- [x] Complete finding-local and repository-wide cleanup discovery; independently process accepted cleanup before parser-boundary documentation.
  - Finding-local review found parser/cache/channel code and current protocol/integration evidence live; no independent net deletion exists.
  - Opaque-claim removal, parser/copy inventory, links, AGENTS guidance, and evidence-limit wording are the F-042 fix.
  - Repository-wide bounded discovery found no high-confidence dead production deletion independent of current or later findings.
  - `_scope`, duplicated helper fields, and unread config fields remain F-043; zeroization, logging, fuzz/tests, broad docs, formatting, and cosmetic churn remain rejected or separately owned.
  - Cleanup is exhausted with no accepted unit and no cleanup commit; the approved parser-boundary documentation may begin.
- [x] Implement and independently review the parser/copy/cache documentation and mandatory review guidance without changing parser behavior or overstating malformed/fuzz evidence.
  - Fix commit: `aedc3750bee8170add7281ee76decd1a8bdd3452` (`docs: document daemon resource parsing`).
  - Security and architecture docs now define the complete backend-open HTTP/base64 buffering, decoded-size and typed parse order, unknown/default/duplicate behavior, conditional env-name validation, canonicality and files/libsecret nonvalidation, 2-MiB versus later 1-MiB publication boundary, typed/cache overlap, Checkout/frame/receiver copies, zeroization limits, and exact finalize checks.
  - `AGENTS.md` makes schema/decode/env/log/clone/cache/copy/buffer changes security-sensitive and names HTTP decode, daemon parser/cache, channel, fd-20 codec, namespace receiver, backend finalize, and secretsd/host publication validation review surfaces.
  - `docs/testing.md` records valid protocol/daemon/integration evidence and explicitly states that no dedicated malformed backend-open daemon matrix, fuzz harness/target/corpus, or recorded fuzz execution exists.
  - Initial implementation reviews required exact typed/cache lifetime ordering and the complete named AGENTS review chain. A separate correction worker fixed both; fresh post-correction review passed. Final evidence review passed the exact four-file unit with no gap.
- [x] Validate, run controller scope/diff/artifact checks, commit coherently, and record F-042 completion/cross-effects before promoting F-043.
  - Exact diff: four approved documentation/guidance files, 93 insertions and 14 deletions, SHA-256 `26cd9b9f5b8d32df134311cfe1157223b679a6e452d4c83f2ac3978792a180e8`.
  - Focused validation passed 24 protocol resource-blob tests and 70 daemon tests (log SHA-256 `477b5984d388e8ebee5ea95fe0c5b7452ea4f0d58370f059d4e696776cda5e79`); the integration runner passed all 105 tests (`ae4c5f7bfc56ddd54a6d794ee8aac7257d8a91fca3ef3c39ea1a753a9d73a7f1`).
  - Static call/copy inventory and absence of dedicated fuzz/property-test surfaces or fuzz directories/corpora passed (log SHA-256 `7205bc62af9b4cc85a46e1d8de16187a77ac536ca7c88262a0cb5502a0e30100`); local links/anchors and obvious one-sentence-per-line scan passed (`9fa1e8064084a67dffeeb851bb63cd59edf077678c8f627426289afbf542cc5a`).
  - The first prose-check script mistook numbered-list prefixes `4.`/`5.` for sentence endings and stopped after all executable validation had passed. The corrected checker excluded Markdown ordered-list markers and passed without a content change; final review classified the first result as a tooling false positive.
  - Controller checks passed exact four-file scope, both opaque daemon claims removed, storage/cookie/id/file uses of “opaque” retained, complete parser/copy/cap inventory, no secret value, `_scope` preserved for F-043, no Rust/Nix/dependency/generated/F-054 change, `git diff --check`, checksum, artifacts, and clean post-commit tree.
  - F-042 is resolved without changing parser behavior. F-009 and F-038 remain resolved; U-009 is narrowed but remains unresolved for comprehensive copy/erasure evidence; F-043, F-054, and every other finding/Opportunity/unresolved item remain unchanged. F-043 is next.
  - Final closure review passed commit/scope, technical claims, historical alignment, review corrections, validation, tooling classification, no-VM rationale, and F-043 promotion. The controller then independently reconfirmed plan-only status, the exact 93/14 commit stat and patch checksum, all four retained log hashes, and `git diff --check`.

### Completed iteration — F-043 remove inert local authority surfaces

F-043 remains directly reproducible at clean HEAD `b1fc1ade186ccf40d95249860283971c0486ae79`.

Three independent dead surfaces imply authority or behavior that does not exist:

- caller-controlled `UNDERWRAP_SCOPE` becomes `WrapperRequest::Read.scope`, crosses the private local wire, and is discarded before backend policy or even production audit use;
- full `DaemonConfig.ns_binary` is generated and validated but unread by `underwrapd`, while the root-generated `/etc/underwrap/instances/<instance>.yaml` sidecar independently selects the capability-bearing namespace helper for `underwrap-exec`;
- six legacy audit helper functions have no production caller and survive only through helper-specific tests or no caller at all.

Consequence: the private ABI, full daemon configuration, and audit API imply nonexistent correlation, helper-selection, approval/store, or secretsd authority and create drift against the actual first-release boundaries.

Security boundary:

- Protected assets: brokered resources, backend policy decisions, and root-controlled selection of the capability-bearing `underwrap-ns` stage.
- Attacker: the untrusted local caller controlling environment, wrapper request bytes, wrapper selector, and invocation variant.
- Scope direction: caller `UNDERWRAP_SCOPE` → local request field → discarded value with no policy/audit effect.
- Helper direction: root-generated instance sidecar `ns_binary` → `underwrap-exec` absolute-path/existence checks → `Command::new` for the namespace stage.
  The full daemon config and daemon response carry no namespace-helper authority.
- Change direction: dead input/config/API is removed and the trusted sidecar boundary becomes singular.
  No authorization, target selection, fd, privilege, or binary-chain mechanism weakens.

Exact scope — remove caller scope:

- Remove `scope` from `WrapperRequest::Read`, its wire encoding/decoding, constructors, destructuring, and daemon call signatures.
- Remove the `UNDERWRAP_SCOPE` environment read and stage-1 Rustdoc mention.
- Remove `scope` from live `read_granted` and `read_denied` audit signatures/events.
  Production currently always emits it as `None`, so no backend decision or successful correlation behavior is removed.
- Pin the exact Real request bytes for `gh-authorized` as `00 0000000d 67682d617574686f72697a6564 00` (request discriminant, string length/body, variant).
  Append the former `Some("session-42")` encoding `01 0000000a 73657373696f6e2d3432` and require exact `trailing bytes in payload` rejection rather than adding a compatibility parser.
- State that old/new mixed `underwrap-exec` and `underwrapd` binaries are intentionally wire-incompatible and must be deployed together, as they already are through one package/module generation.

Exact scope — make the sidecar the sole namespace-helper authority:

- Remove `DaemonConfig.ns_binary`, its misleading comment, validation entry, typed constructors, YAML fixtures, and path-negative test.
- Remove only `runtimeConfig.ns_binary` from `nix/modules/underwrap.nix`.
  Retain `instanceSidecar.ns_binary`, `nsBinaryPath`, the generated sidecar file, `underwrap-exec` sidecar parsing/default/absolute/existence checks, and `Command::new(ns_binary)` unchanged.
- Remove `ns_binary` from `examples/daemon-config.yaml` and from the full generated-daemon path inventory in `docs/configuration.md`.
  Describe `/etc/underwrap/instances/<instance>.yaml` as the root-generated strict entry-stage helper authority.
- Keep current top-level `DaemonConfig` unknown-field behavior unchanged.
  A stale hand-written `ns_binary` key may be ignored after typed-field removal, but it grants no authority and no compatibility branch reads it.
- Remove the obsolete key from integration and standalone VM daemon fixtures.
- Extend the VM's generated-config assertions to parse both JSON-compatible YAML files and require the full daemon config to omit `ns_binary` while the sidecar has exactly `socket`, `ns_binary`, `dbus_daemon_binary`, and `secretsd_binary` with the fixed helper path.
  Existing deployed wrapper success remains the end-to-end helper-selection evidence.
- In `docs/architecture.md`, state that stage 1 loads its helper paths from the root-generated sidecar and that the daemon request carries only wrapper selector plus variant.

Exact scope — remove unreachable audit helpers:

- Delete `secret_stored`, `store_denied`, `approved`, `secretsd_read`, `secretsd_write`, and `secretsd_delete`.
- Preserve `read_granted`, `read_denied`, `auth_denied`, `resource_checkout`, `resource_checkin`, `resource_usage_signal`, and `wrapper_write_denied` with current event names and non-scope fields.
- Rewrite audit tests to exercise every live helper, preserve identifier-only/secret-value hygiene evidence, and assert no `scope` field.
- Keep current `secret`/`store_denied` vocabulary and writeback comments for F-052 rather than renaming event fields in this finding.

Expected implementation/test/documentation allowlist — 23 files:

- `crates/underwrap-protocol/src/message/wrapper.rs`
- `crates/underwrap-protocol/src/message/tests.rs`
- `crates/underwrap-protocol/src/config.rs`
- `crates/underwrap-protocol/src/config/tests.rs`
- `crates/underwrap-exec/src/main.rs`
- `crates/underwrapd/src/wrapper_flow/mod.rs`
- `crates/underwrapd/src/wrapper_flow/read.rs`
- `crates/underwrapd/src/audit.rs`
- `crates/underwrapd/src/audit/tests.rs`
- `crates/underwrapd/src/lib/tests.rs`
- `crates/underwrapd/src/main/tests.rs`
- `crates/underwrapd/src/test_support.rs`
- `crates/integration-tests/src/harness/mod.rs`
- `crates/integration-tests/src/tests/mod.rs`
- `crates/integration-tests/src/tests/daemon.rs`
- `crates/integration-tests/src/tests/store_read.rs`
- `crates/integration-tests/src/tests/wrapper_writeback.rs`
- `nix/modules/underwrap.nix`
- `nix/tests/test.py`
- `examples/daemon-config.yaml`
- `docs/configuration.md`
- `docs/architecture.md`
- `docs/testing.md`

The active plan changes separately for workflow evidence only.

Approved cleanup 1 — reuse the canonical dynamic-store path helper:

- Remove one-use `TestDaemon::store_data_dir` from `crates/integration-tests/src/harness/mod.rs`.
- Change only `crates/integration-tests/src/tests/rotation.rs` to call existing `store_dir_for`.
- The rotation test's admin-created `store_<20 hex>` id cannot equal the seeded `test` special case, so both helpers resolve exactly `stores_data_root/<store_id>`.
- This removes one redundant private test-harness API, preserves F-003 rotation-response-loss evidence, and affects no F-id.
- Cleanup allowlist: exactly those two integration-test paths.
- Cleanup validation: exact rotation test through the packaged runner, complete 105-test runner, focused denied-warning integration Clippy, rustfmt, reachability/scope/diff checks, and no generated artifact.

Expected cross-effects:

- F-031 remains resolved with the full daemon grammar narrowed by removal of `ns_binary` and the fixed root-generated sidecar retaining namespace-helper authority.
- F-033 remains resolved; the local wrapper request ABI becomes smaller without changing protocol feature profiles.
- O-010 is narrowed by legacy-helper removal but remains separately unresolved because F-052 owns remaining audit vocabulary/comments.
- F-042 parser/cache behavior, F-052 vocabulary, F-054 log-hygiene evidence, and every other finding/Opportunity/unresolved item remain unchanged.

Non-goals:

- no approval/session/correlation scope feature, replacement field, logging of caller scope, backend HTTP scope, permission/policy mode, audit redesign/vocabulary rename, event retention stub, or compatibility parser/branch;
- no top-level `DaemonConfig` unknown-field denial, keygen partial-config parse, sidecar schema/path/default, namespace-helper path, `underwrap-exec → underwrap-ns` binary chain, daemon-chosen target, fd 3/fd 20/fd 21, ExecBundle, target identity, capability, or privilege change;
- no dependency, Cargo/OpenAPI/resource schema, backend behavior, SPA, unrelated Nix option/service, F-044 environment change, F-052 audit wording, F-054 evidence expansion, historical record edit, later finding, Opportunity, or unresolved-item implementation.

Validation commands and evidence:

- exact static negative inventory for `UNDERWRAP_SCOPE`, request/audit scope fields, `_scope`, `scope.as_deref`, six dead helpers, and full-daemon `ns_binary` constructors/fixtures/docs
- exact positive inventory for `instanceSidecar.ns_binary`, underwrap-exec sidecar parse/absolute/existence/exec path, and all seven live audit helpers/callers
- exact protocol request-byte and legacy trailing-field rejection tests
- `cargo test --locked -p underwrap-protocol`
- `cargo test --locked -p underwrap-protocol --no-default-features --features local-abi`
- `cargo clippy --locked -p underwrap-protocol --no-default-features --features local-abi -- -D warnings`
- `cargo test -p underwrap-exec`
- `cargo test -p underwrapd`
- `cargo test -p underwrap-keygen`
- `./crates/integration-tests/run`
- build exact Nix `web-build`, then run bounded non-integration workspace tests and workspace denied-warning Clippy
- `cargo fmt -- --check`
- `nix build .#checks.x86_64-linux.frontend-security-assertions-eval`
- `nix build .#checks.x86_64-linux.examples-nixos-eval`
- `nix build .#checks.x86_64-linux.workspace-test`
- `nix flake check --all-systems --no-build`
- `nix build .#checks.x86_64-linux.vm-test`, requiring complete `ALL TESTS PASSED` and exact generated daemon/sidecar authority assertions
- direct source/docs review that the wire/config removals add no compatibility branch, no dead audit helper survives, F-052 vocabulary is unchanged, and no secret enters logs/docs
- `git diff --check`

The VM is mandatory because the NixOS module's generated daemon/sidecar split and the deployed `underwrap-exec → underwrap-ns` helper authority are directly in scope.

- [x] Revalidate F-043 against current request scope flow, audit output/helpers, daemon versus sidecar namespace-helper paths, config/tests/examples/docs/Nix/VM fixtures, keygen parsing, F-031/F-033/F-052 effects, O-010, and historical report/handover. Evidence: direct CodeGraph/source/test/doc search plus two focused design/evidence reviews on 2026-08-30; all three inert surfaces persist and exact current consumers are enumerated above.
- [x] Obtain fresh independent approval of all three removals, sidecar authority, private wire/config/audit effects, 23-file allowlist, non-goals, cross-effects, validation, and mandatory VM before cleanup discovery. Evidence: implementation/security review passed the complete scope; evidence review required exact wire/trailing vectors and standalone F-033 local-ABI test/Clippy preservation. The plan incorporated those additions, an interrupted delta review found no blocker, and a bounded final disposition passed on 2026-08-30 with implementation evidence pending.
- [x] Complete finding-local and repository-wide cleanup discovery; independently process accepted cleanup before F-043 removals.
  - Finding-local discovery accepted cleanup 1 exactly as recorded above; independent review proved dynamic-store path equivalence, net-deletion value, reversibility, and no F-id effect.
  - Repository-wide bounded discovery found no other high-confidence private dead production deletion independent of current or later findings.
  - Scope/config/audit removals are the F-043 fix; F-044/F-052/F-054 work, broad refactors, dependencies, formatting, and cosmetic churn are rejected.
  - Cleanup is exhausted after cleanup 1.
- [x] Implement, independently review, validate, and commit cleanup 1 before F-043 removals.
  - Commit: `03a7060293bd1c65b20cdf5d8614cb7d76ecec65` (`test: reuse dynamic store path helper`).
  - Exact diff: two approved paths, one insertion and five deletions, SHA-256 `cffc97e2adc955fb29251dc9bcb8022789d7e2bd4355a77e623d10bab7b352e2`; generated dynamic store ids take `store_dir_for`'s identical non-`test` branch.
  - Fresh actual-diff review passed path equivalence, F-003 evidence preservation, runner invocation, scope, and no F-id effect.
  - The exact rotation test passed (`67eb59182a2af392b87b1f3c4956cd1a3054c7ce8c61d4c4daf73d05d04303c4`), followed by all 105 integration tests (`14834bc1c56ff964bb49ae322c1175848dd3d9c9de4ebfce1336234d24f7dc4d`).
  - The first controller Clippy invocation omitted the backend's required web bundle and stopped in its build script after tests had passed (`ed5e3da2e664485194b3f6db7c6ed2e44ec7903aa12e3b51e63d99857fd618ab`). With exact Nix web artifact `/nix/store/q6j5j105c1y9ilcppwaph4y9j36wirrz-underwrap-web-dist-0.1.0` (`33fec8fdf10471630c29c0c98e6ebbe21fce3ccb07019bab8ff0fab74378da14`), focused denied-warning Clippy and rustfmt passed (`17326ef30736171a652c85bb70a12b4e3182b42862d045dd9738c9182aba6a7c`).
  - Controller checks passed exact two-file scope, helper absence, path-equivalence source, `git diff --check`, checksum, no artifacts, and clean post-commit tree. The missing-web result is prerequisite provenance, not an implementation/test failure.
- [x] Implement and independently review the exact dead-surface removals and authority evidence without changing the wrapper chain or adjacent findings.
  - Fix commit: `895a6d506e54a79ce8dcd0f4fca246cb1c51ace6` (`refactor: remove inert local authority surfaces`).
  - `WrapperRequest::Read` now carries only wrapper selector and variant; stage 1 no longer reads `UNDERWRAP_SCOPE`, daemon/audit plumbing has no scope field, exact Real bytes are pinned, and the retired `Some("session-42")` suffix fails with exact trailing-byte rejection.
  - Full `DaemonConfig` and generated daemon YAML no longer contain `ns_binary`. The fixed Nix sidecar retains exactly the four entry-stage keys, and `underwrap-exec` retains its namespace-helper default/parse/absolute/existence/`Command::new` path unchanged.
  - Six production-unreachable audit helpers are removed. Tests exercise all seven live helpers, preserve event names and F-052 vocabulary, require seven events with no scope field, and retain identifier-only/value-hygiene evidence.
  - The implementation worker exhausted its runtime after completing all 23 edits and substantial validation. Two fresh source/evidence reviews found no implementation blocker but withheld exact-diff PASS because their tools lacked Git/log access; after controller staged and verified the exact diff and all evidence, a bounded final review returned PASS.
- [x] Validate, run controller scope/diff/security/artifact checks, commit coherently, and record F-043 completion/cross-effects before promoting F-044.
  - Exact staged diff: 23 approved paths, 106 insertions and 130 deletions, SHA-256 `c23f184224f47ffb02e4cc1c628af15d60d81f67cb6c201ddc9925d3a7165673`.
  - Focused Rust validation passed 119 full and 63 isolated local-ABI protocol tests plus local denied-warning Clippy, 14 entry-stage tests, 70 daemon tests, and 27 keygen tests (log SHA-256 `e57e32481eac7e1d0de94a3576f95224940ddd1b6fe51572655bdf4f07e68d56`).
  - The packaged integration runner passed all 105 tests (`89eecaed1b484a1bbcfbbfc7eee5629b24601d9506f5619fc54139a31c881b43`). Bounded workspace tests passed with exact Nix web artifact `/nix/store/q6j5j105c1y9ilcppwaph4y9j36wirrz-underwrap-web-dist-0.1.0` (`1cd23f2278f130b6b36e6c36f595ac75a55336e52a04695dd9ca91fa558f87be`; prerequisite `33fec8fdf10471630c29c0c98e6ebbe21fce3ccb07019bab8ff0fab74378da14`), and rustfmt plus full workspace Clippy passed (`f4403412cb20dc7d8a0a94c08c9c8853e4bac9b3e8489158834f30512311ed97`).
  - Frontend security and example evaluators passed with explicit markers (`d67255cc0d73a4c265c1b123753837a4530cbfecb877a8b1567615ac7546e633`, `4eeaed70a5c09da580bc1594d17259cbd61a67567e73d5b66b261ec0fa5d4e69`), Nix workspace-test passed (`545dd9b4cbd8657b4f7eddafa177b5b24d492e5cab2bdc348ad542d7153c04fc`), and all-system evaluation passed (`c0943628938695fb7e48fa47b7376d0f4dfd17e4bb249c9254036d683fd0e57c`).
  - The complete NixOS VM derivation passed with `machine: ALL TESTS PASSED` and `test script finished in 165.53s` (explicit derivation log `76d9b566fe63cbcdebfba91065e44a34227fb1bc4ff23759b9466db62ba06c3b`; complete build log `f74df9a505fa14d87503578b3c556029c626e21dbe2fc077d42ac49d3af3a512`). It directly proves daemon omission, exact sidecar keys/path, and unchanged deployed wrapper-chain success.
  - Static negative/positive authority and live-audit inventories plus Python AST checks passed (`04670ddbbf2a1c9886fd49b10c515ccd93b7486a01577904bc030f83d875aba9`); documentation links passed (`db0d297de242af1fcecca6a7e9508c82bd55ed8f83cb04f3b72a464dbd33fb5a`).
  - Controller checks passed exact 23-file scope, no compatibility/unknown-field branch, old/new wire fail-closed reasoning, only one Nix `ns_binary` generation site, retained sidecar/exec authority, six-dead/seven-live audit inventory, F-052 vocabulary, no secret value/dependency/OpenAPI/generated/unrelated change, `git diff --check`, checksum, removal of `result`/bytecode artifacts, and clean post-commit tree.
  - F-043 is resolved. F-031 remains resolved with its daemon path inventory narrowed and root-generated sidecar authority retained; F-033 remains resolved with the smaller local ABI; O-010 is narrowed but remains separate; F-042, F-052, F-054, and every other finding/Opportunity/unresolved item remain unchanged. F-044 is next.
  - Final closure review passed ordered history, both stats/checksums, cleanup equivalence, wire/config/audit semantics, review/controller chronology, complete validation including VM, cross-effects, and F-044 promotion. The controller then independently recomputed both commit hashes/stats, every retained log hash, plan-only status, and `git diff --check`.

### Completed iteration — F-044 forced target-environment precedence

F-044 remains directly reproducible at clean HEAD `8ddd2851dea29d7db2f322523b1f8d337bd61250`.

The shared target-environment authority defines `HOME`, `XDG_RUNTIME_DIR`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, and `XDG_STATE_HOME` as forced names.
`validate_env_name` reserves `HOME` but not the five XDG names.

`build_target_env` correctly forces/defaults those paths, allows trusted wrapper `envSet` to override `HOME` and four base XDG directories, and never lets config override `XDG_RUNTIME_DIR`.
It then validates and inserts projected resource env entries last, so a resource XDG entry currently replaces the trusted path.

Consequence: trusted but contradictory operator data or directly injected malformed state can redirect target configuration/cache/state/runtime discovery away from the documented target identity and wrapper configuration.

Security boundary:

- Protected asset: deterministic target home, configuration, data, cache, state, and runtime-directory selection.
- Failure source: trusted operator resource data or malformed directly injected/backend state.
  The local caller cannot administer resource values and gains no new privilege through this correction.
- Boundary direction: backend resource env map → shared env-name validation → pure target-environment composition → configured target process.
- Change direction: exact forced-name collisions fail closed before insertion and before target/projection effects.
  Ordinary env/resource precedence and every identity/authorization mechanism remain unchanged.

Exact production scope:

- Make `underwrap_protocol::resource::is_reserved_env_name` consult `target_env::is_forced_target_env_name` as the one Rust authority for all six exact names.
  Remove the now-redundant literal `HOME` from the older reserved-name match and retain `PATH`, `USER`, `LOGNAME`, `SHELL`, `DBUS_SESSION_BUS_ADDRESS`, `SSH_AUTH_SOCK`, `LD_*`, and `UNDERWRAP_*` behavior unchanged.
- Do not modify production `underwrap-ns::env::build_target_env` ordering.
  Its existing `validate_env_name` call occurs before UTF-8 decode and insertion, so the shared change makes projected forced-name entries fail before they can override trusted values.
- Do not skip or silently discard a projected forced-name entry.
  An env-enabled wrapper fails closed with the existing identifier-only reserved-name diagnostic.
- Preserve the ordinary valid-input precedence `resource env > envSet > passthrough` for non-forced names.
- Preserve conditional projection behavior.
  A forced-name entry in the resource does not block a files/libsecret-only wrapper whose `env_projection` is false because daemon and namespace env validation remain conditional.
- Add no migration or compatibility path for previously injected malformed state.
  Shared per-key admin read/write/delete/rename validation rejects the invalid key or destination, and F-011 content blockers also prevent normal resource deletion while that env entry remains.
  The normal admin API therefore fails closed rather than repairing this outside-contract state.
  Recovery requires a quiesced trusted backend-host restoration/replacement of known-good canonical store state or rebuilding the affected store; this finding adds no automated repair procedure.

Exact test scope:

- In protocol tests, iterate all six forced names through both `validate_env_name` and `validate_secret_key(ResourceSlot::Env, ...)`, require the existing value-free reserved-name error, and accept an exact `${name}_APP` near miss for each.
- In namespace env tests, inject each forced name through `ResourceBlob.env` with projection enabled and require fail-closed pure-planning rejection before value decode/insertion with no secret value in the diagnostic.
  Pair each with accepted `${name}_APP` entries.
- Keep/prove trusted forced values: configured `HOME` plus four base XDG overrides, dedicated `XDG_RUNTIME_DIR`, and ambient forced-name suppression.
- Add an ordinary non-forced collision proving resource env still overrides `envSet`.
- Add `env_projection = false` evidence proving a forced-name resource entry is ignored while trusted forced values remain.
- In backend admin tests, matrix all six names through env write and rename-destination handlers.
  Require HTTP 400 `invalid_request`, the value-free shared reason, complete `ResourceBlob` equality after every rejection, no material in the response, and valid near-miss write/rename recovery.
- In real backend/daemon integration, use authenticated admin requests to reject all six names without changing the canonical blob.
  Directly inject each otherwise canonical invalid blob, require an env-enabled daemon read to return `ErrorCode::Internal` with identifier-only/no-value text, and perform a bounded SCM_RIGHTS receive attempt that observes EOF/no descriptor rather than inferring channel absence.
  Require forced-name per-key admin deletion and normal resource deletion to fail closed, proving the malformed state is outside the admin repair surface.
  Replace each invalid blob through the trusted direct-storage test seam with a near-miss blob and prove the same daemon/backend recovers to `ReadOk`; this simulates host restoration and is not documented as an operator API.
  Add a files-only/env-disabled wrapper case proving the conditional F-042 parser behavior remains.
  Do not claim runtime-open has zero backend effects because pair usage metadata may still change.

Expected source/test/documentation allowlist:

- `crates/underwrap-protocol/src/resource.rs`
- `crates/underwrap-protocol/src/resource/tests.rs`
- `crates/underwrap-ns/src/env/tests.rs`
- `crates/underwrap-backend/src/admin_api/secrets/tests.rs`
- `crates/integration-tests/src/tests/store_read.rs`
- `docs/security.md`
- `docs/configuration.md`
- `docs/testing.md`

The active plan changes separately for workflow evidence only.

Approved cleanup 1 — remove unused `ResourceSlot` string parsing trait:

- Delete only `impl std::str::FromStr for ResourceSlot` from `crates/underwrap-protocol/src/resource.rs`.
- Preserve the live `ResourceSlot::parse` method used by backend admin route parsing, Serde/Utoipa derives, names, routing, and all wire behavior.
- Static search finds no typed/inferred `.parse()`, trait bound, test, doc, example, or dynamic trait-object/registry consumer.
- This removes seven lines of unused public API with `()` error semantics; hypothetical external use remains unknown but non-blocking under the greenfield/no-published-compatibility posture.
- Cleanup allowlist: exactly `crates/underwrap-protocol/src/resource.rs`.
- Cleanup validation: full and isolated local-ABI protocol tests plus denied-warning Clippy, workspace consumer check, rustfmt, exact reachability/diff/artifact checks, and no F-id effect.

Expected cross-effects:

- F-006 remains resolved because namespace rejection remains in pure environment planning before projection effects.
- F-031 remains resolved; shared forced-name authority is reused rather than duplicated.
- F-042 remains resolved with its conditional env-name parser behavior unchanged except for the expanded exact reserved set.
- F-054 remains unchanged; new diagnostics/assertions contain identifiers and reasons but no values.
- Every other finding, Opportunity, and unresolved item remains unchanged.

Non-goals:

- no reordering of ordinary passthrough/envSet/resource precedence, silent skip, env value transformation, target identity/path derivation, envSet override list, XDG default, runtime-directory provisioning, path grammar, clock, locale/proxy/passthrough behavior, or non-forced reserved-name change;
- no resource schema/OpenAPI/SPA/Nix option/module, backend storage format, automatic migration/repair, privileged chain, namespace/mount, fd, capability, target UID, policy/writeback, authorization, dependency, or generated artifact change;
- no F-045 alias validation, F-050 fixture, F-052 audit wording, F-054 log-evidence expansion, historical record edit, broad environment refactor, later finding, Opportunity, or unresolved-item implementation.

Validation commands and evidence:

- `cargo test -p underwrap-protocol resource::tests`
- `cargo test --locked -p underwrap-protocol --no-default-features --features local-abi`
- `cargo test -p underwrap-ns env::tests`
- `cargo test -p underwrap-backend admin_api::secrets::tests`
- `./crates/integration-tests/run tests::store_read::forced_target_env_names_fail_closed_and_recover -- --exact`, then the unchanged complete runner
- full protocol, namespace, backend, and daemon tests
- `cargo clippy --locked -p underwrap-protocol --no-default-features --features local-abi -- -D warnings`
- build exact Nix `web-build`, then run bounded non-integration workspace tests and workspace denied-warning Clippy
- `cargo fmt -- --check`
- `nix build .#checks.x86_64-linux.workspace-test`
- exact positive/negative inventory proving all six shared forced names reject, near misses and ordinary precedence remain, and no duplicate six-name list was introduced
- direct boundary review across daemon HTTP/open parser, channel/cache, fd-20 codec/receiver, namespace env builder, backend admin validation, and diagnostics
- direct docs/link/one-sentence/scope/secret checks
- `git diff --check`

A VM run is not required because no NixOS module, deployed identity, namespace/mount setup, capability, privileged chain, fd topology, or wrapper-service configuration changes.
The affected shared lexical rule and pure environment planner are exercised directly, while real backend/daemon integration covers authenticated ingress, malformed-state rejection, and recovery.

- [x] Revalidate F-044 against current shared forced-name policy, resource validation, environment composition order, daemon conditional parse, backend admin ingress, tests/docs, F-006/F-031/F-042/F-054 effects, and historical report/handover. Evidence: direct CodeGraph/source/test inspection plus two focused design/evidence reviews on 2026-08-30; all five XDG names remain admissible resource env keys and no collision regression exists.
- [x] Obtain fresh independent approval of the one-line shared production change, fail-closed precedence/legacy behavior, exact test matrix, allowlist, non-goals, cross-effects, validation, and no-VM rationale before cleanup discovery. Evidence: implementation/security review passed. Evidence review required direct bounded no-SCM_RIGHTS observation, an exact focused integration command, and isolated local-ABI denied-warning Clippy; the plan incorporated all three and fresh final re-review passed on 2026-08-30 with implementation evidence pending.
- [x] Complete finding-local and repository-wide cleanup discovery; independently process accepted cleanup before forced-name enforcement.
  - Finding-local discovery accepted cleanup 1 exactly as recorded above; independent review verified no consumer, behavior preservation, public-API greenfield posture, separate hunk, validation, and no F-id effect.
  - Repository-wide bounded discovery found no other high-confidence private dead production deletion independent of current or later findings.
  - Forced-name authority/tests/docs are the F-044 fix; env reorder/refactor, F-045/F-050/F-052/F-054, dependencies, formatting, and cosmetic churn are rejected.
  - Cleanup is exhausted after cleanup 1.
- [x] Implement, independently review, validate, and commit cleanup 1 before forced-name enforcement.
  - Commit: `49ee3a7d06b39cbfd3ac8cada25e2fd602a3efce` (`refactor: remove unused ResourceSlot FromStr`).
  - Exact diff: one approved file, seven deletions, SHA-256 `98aaaaddc3828f74d2f26092895bba724286e60a29da706154d0d456b169f782`; live `ResourceSlot::parse` and every runtime/Serde/schema behavior remain.
  - Initial independent review confirmed reachability and semantics but withheld on unavailable Git/test evidence; after controller exact diff and validation, final re-review passed with no gap.
  - Validation passed 119 full and 63 isolated local-ABI protocol tests, full/local denied-warning Clippy, workspace all-target consumer check, and rustfmt (protocol log SHA-256 `7a93390d0f811df785d2c0ae4d33b5542cd34b70a94bac523b7580be878dac74`; workspace log `8a2933ae25e59188be988ea6218274cb341f13d8bc4b533f57b12ececd4be8b3`; exact Nix web prerequisite `33fec8fdf10471630c29c0c98e6ebbe21fce3ccb07019bab8ff0fab74378da14`).
  - Controller checks passed exact trait-consumer absence, live parser use, one-file scope, `git diff --check`, checksum, no artifact, and clean post-commit tree. External downstream use remains unknown and accepted under greenfield posture; no F-id changed.
- [x] Implement and independently review the shared forced-name rejection and complete unit/integration evidence without changing ordinary precedence.
  - Fix commit: `d3ba7e9d1ceba9ee8fc01e1bc72a8087ea54f161` (`fix: preserve forced target environment paths`).
  - `is_reserved_env_name` now delegates the exact six forced HOME/XDG names to `target_env::is_forced_target_env_name`; the redundant `HOME` literal is gone and every other reserved rule remains unchanged.
  - Protocol/namespace/backend/integration matrices cover all six names, exact near misses, trusted forced values, ordinary resource precedence, env-disabled conditional behavior, value-before-decode diagnostics, authenticated write/rename/delete rejection, blob equality, content-blocked resource deletion, direct malformed-state daemon rejection, bounded EOF/no-SCM_RIGHTS, test-only trusted replacement, and recovery.
  - Initial production/evidence review found one major documentation error: normal resource deletion cannot repair malformed forced-name state because F-011 content blockers preserve it. The plan and docs were corrected to require normal per-key/resource-delete fail-closed evidence and quiesced trusted-host known-good restoration/rebuild; a separate correction worker added that integration proof. Fresh post-correction and final evidence reviews passed the exact eight paths with no gap.
- [x] Validate, run controller scope/diff/security/artifact checks, commit coherently, and record F-044 completion/cross-effects before promoting F-045.
  - Exact staged diff: eight approved paths, 541 insertions and 38 deletions, SHA-256 `4854b08ef4c58a27013ed6d43f3654552a480cc2c699b7ebeb60829cf4c1eb78`.
  - Protocol validation passed 15 focused resource tests, 120 full tests, 64 isolated local-ABI tests, and isolated denied-warning Clippy. Namespace validation passed 15 focused env tests and all 125 tests under the supported four-thread setting (combined pre-prerequisite log SHA-256 `6efa8409557f2fdd8e731aba761f59eb2573a597a3f336fefc887320ab8488f8`).
  - The first controller backend command omitted the required SPA bundle and stopped in the backend build script after protocol/namespace validation. With exact Nix web artifact `/nix/store/q6j5j105c1y9ilcppwaph4y9j36wirrz-underwrap-web-dist-0.1.0` (`33fec8fdf10471630c29c0c98e6ebbe21fce3ccb07019bab8ff0fab74378da14`), seven focused admin-secret tests, 259 backend tests plus 3 binary and 5 OpenAPI tests, and 70 daemon tests passed (`b356095ef176b88fee4e2776ee6572128fa4676497b63899844a834e8729df9e`). This is prerequisite provenance, not an implementation/test failure.
  - The exact F-044 integration test passed (`4656217e3c830c1f3414d7ebc34ed20326cf8baabbf9dd3e5b9cfaa3f9f9f655`), followed by all 106 integration tests (`4617e41917bd13587bfad239efa72af766d1f2e88823f679cdfcd4939b8e88f8`).
  - Bounded workspace tests passed (`f0a126f9310c5114439f1032111a9abb3934a4e658a789b40bb2bce4ab5520cb`); rustfmt and workspace denied-warning Clippy passed (`cbdeb3192289045f42d737e86aa1852bf02097fc3f472179e7e1f7872f3467ce`); Nix workspace-test passed (`d3649d9c761b4f0d042f3d2cbb2c175114c35a705f0a5a9b2b10e321a44005a7`); docs links passed (`6eaab658ff0b4408ce82b86d3790fea753711a52bec2599b7ceec81f1dbb329f`).
  - The implementation worker reported the existing unrestricted-thread namespace fd-number-reuse interference. The exact affected path and complete supported four-thread namespace/workspace runs passed; final review classified it as harness concurrency interference, not product evidence.
  - Controller checks passed exact eight-path scope, one shared forced-name reference and no duplicate XDG list, unchanged `env.rs`/`target_env.rs`, identifier-only/no-value diagnostics, normal repair-surface limits, no Nix/OpenAPI/schema/dependency/generated/F-054 change, `git diff --check`, checksum, artifacts, and clean post-commit tree.
  - F-044 is resolved. F-006, F-031, and F-042 remain resolved; F-054 and every other finding/Opportunity/unresolved item remain unchanged. F-045 is next.
  - Final closure review passed ordered history, both stats/checksums, forced-name semantics, recovery correction, validation and prerequisite/transient classifications, controller checks, no-VM rationale, cross-effects, and F-045 promotion. The controller then independently recomputed both commit stats/hashes, every retained log hash, plan-only status, and `git diff --check`.

### Completed iteration — F-045 early SetAlias graph validation

F-045 remains valid but narrowed at clean HEAD `986b5e47065371246759c0292c11f3525bec65b0`.

F-008 strict host publication validation rejects malformed collection/alias segments and dangling alias references.
F-012 candidate commit-after-ACK therefore prevents those malformed candidates from entering accepted live memory or later backend writeback.

`SecretService.SetAlias` still performs no early alias-name, exact collection-path, or target-existence validation.
Most invalid inputs produce a candidate that the host later rejects with generic `Failed`, but an unexpected non-root collection path fails `strip_prefix`, becomes `None`, and is persisted as a valid alias removal after ACK.

Consequence: untrusted target D-Bus input can remove an existing alias by supplying an unrelated object path, while other malformed/dangling requests cross publication unnecessarily and report the wrong D-Bus error class.

Security boundary:

- Protected asset: integrity and referential consistency of the accepted in-memory and persisted libsecret alias graph.
- Attacker: the configured target's D-Bus client input, treated as untrusted for robustness and storage integrity even though the target is trusted with its own projected secret.
- Boundary direction: `Service.SetAlias(name, collection_path)` → early segment/exact-path/existence checks → candidate mutation → canonical publication → exact host ACK → accepted live store → later Checkin.
- Change direction: malformed or dangling input rejects with `org.freedesktop.DBus.Error.InvalidArgs` before candidate creation/publication, while `/` clear and exact existing collection paths retain their behavior.
  F-008/F-012 remain defense in depth and no authorization boundary weakens.

Exact implementation scope:

- Introduce one private generic Secret Service path-segment validator used by the existing collection-name validator and the new SetAlias alias/target checks.
  Preserve the exact current rule: nonempty, at most 128 bytes, and only ASCII letters, digits, or underscore; leading digits and underscores remain legal.
- Preserve CreateCollection's existing behavior and accepted client names while routing its collection-name check through that helper.
  Do not add early validation for CreateCollection's optional alias in this finding.
- Validate the SetAlias alias name before interpreting the collection path or acquiring the store lock.
  Invalid alias-name input has first-error precedence.
- Treat exact root path `/` as the sole clear operation.
- For a non-root path, require the exact `${BASE_PATH}/collection/<one-valid-segment>` shape.
  Reject another prefix, the collection container path, nested/item paths, extra segments, or a malformed segment with `InvalidArgs`; never reinterpret them as clear.
- Acquire the store write lock only after alias/path shape validation.
  For a set operation, require the target collection id to exist in that same locked store before candidate creation.
- Return `InvalidArgs` for a missing collection with an identifier-only/value-free message and no publication.
- Only after all checks pass, call the existing `mutate_and_publish` path and preserve exact candidate, canonicalization, ACK/REJECT, zeroization, and commit behavior.
- Preserve idempotent clear of a missing alias, setting an alias to its current target, ordinary `ReadAlias`, metadata trace policy, and all valid D-Bus signatures/paths.

Exact test scope:

- Add a direct shared-segment-validator matrix: common client names, leading digit/underscore, and exactly 128 ASCII bytes succeed; empty, 129 bytes, slash/nested, dash, dot, space, newline/control, and non-ASCII reject.
  Use a D-Bus-representable control such as newline rather than NUL for alias-handler cases.
- Retain CreateCollection name/idempotence tests through the shared helper.
- Keep handler object-path cases separate from direct validator cases.
  Require `ObjectPath::try_from` to succeed before every SetAlias path invocation.
- Add production-handler path rows for unexpected non-root prefix, collection container path, nested/item path, extra segment, an otherwise valid 129-byte segment, and a syntactically valid exact path naming a missing target.
- Add malformed alias-name rows against an otherwise valid existing target, and invalid alias plus malformed/dangling target rows proving alias-name-first precedence.
- For every prevalidation row, create a fresh service with an empty publication script and a secret-bearing store containing one unique synthetic sentinel.
  Require exact `InvalidArgs`, complete store equality, `script.finish().is_empty()`, and diagnostic absence of the sentinel.
  Seed a retained alias for every path-shape row and prove no request clears or changes it.
- Retain static source inventory proving all prevalidation branches precede `mutate_and_publish`.
- Add valid ACK cases for ordinary and leading-digit/underscore aliases, an exact 128-byte alias, and an exact existing collection path whose one collection segment is 128 bytes.
  Each boundary case must prove one publication and matching accepted/published alias state.
- Add ACK-backed root-path clear, idempotent clear of a missing alias, and same-target set.
- Retain existing host REJECT, wrong-generation, and reject-then-ACK tests proving F-012 accepted-state isolation after valid prevalidation.

Expected source/test/documentation allowlist:

- `crates/underwrap-secretsd/src/dbus_service/service.rs`
- `crates/underwrap-secretsd/src/dbus_service/service/tests.rs`
- `docs/secretsd-security.md` — require early alias/path/existence `InvalidArgs` before candidate publication and retain strict host validation as defense in depth
- `docs/testing.md` — distinguish direct shared-validator tests from typed production-handler tests and disclaim a D-Bus wire-harness proof

The active plan changes separately for workflow evidence only.

Expected cross-effects:

- F-008 remains resolved; strict host validation stays as defense in depth for all publication producers.
- F-012 remains resolved; candidate commit-after-ACK and zeroization behavior are unchanged, while malformed requests no longer create candidates.
- F-039 remains resolved; alias mutation does not change item/collection timestamp semantics.
- F-046 wrong-typed CreateItem property handling remains unresolved and unchanged.
- U-004 is narrowed but remains unresolved for broader Secret Service draft conformance.
- Every other finding, Opportunity, and unresolved item remains unchanged.

Non-goals:

- no CreateCollection optional-alias change, CreateItem property typing, item/collection identifier-generation rule, object registration, alias timestamp, D-Bus signature/path namespace, ReadAlias behavior, session/locking/prompt behavior, or broader draft conformance change;
- no store-model alias API validation, host strict-validation removal, candidate/ACK/zeroization/publication framing, backend resource format/writeback, migration/repair, automatic graph cleanup, path sanitization, compatibility branch, dependency, Nix/VM, privileged chain, or security-boundary weakening;
- no value-bearing error/log field, F-046, F-052 vocabulary, F-054 log-evidence expansion, historical record edit, broad refactor, later finding, Opportunity, or unresolved-item implementation.

Validation commands and evidence:

- focused shared segment and SetAlias prevalidation/valid tests
- `cargo test -p underwrap-secretsd dbus_service::service::tests`
- `cargo test -p underwrap-secretsd`
- `cargo test -p underwrap-secretsd-store`
- `cargo test -p underwrap-protocol resource_blob::tests`
- `cargo test -p underwrap-ns publication::tests`
- build exact Nix `web-build`, then run bounded non-integration workspace tests and workspace denied-warning Clippy
- `./crates/integration-tests/run`
- `cargo fmt -- --check`
- `nix build .#checks.x86_64-linux.workspace-test`
- exact source/test inventory proving malformed shape/name cases reject before lock, all malformed and dangling cases reject before candidate creation/publication, and all valid cases use the existing candidate path
- direct docs/link/one-sentence/scope/secret checks
- `git diff --check`

A VM run is not required because no NixOS/module, identity, namespace, capability, privileged chain, D-Bus signature, object registration, or target-visible valid-input behavior changes.
Direct production-handler tests with scripted publication outcomes cover the changed untrusted input boundary, while protocol/namespace/integration suites cover unchanged downstream validation and writeback.

- [x] Revalidate F-045 against current SetAlias parsing, store alias mutation, strict host validation, F-012 candidate commit/zeroization, tests/docs, F-008/F-039/F-046/U-004 effects, and historical report/handover. Evidence: direct CodeGraph/source/test inspection on 2026-08-30; malformed/dangling candidates no longer poison accepted state, but unexpected non-root paths can still persist unintended removal and early `InvalidArgs` coverage is absent.
- [x] Obtain fresh independent approval of early validation order, exact path/segment/existence rules, error class, valid/invalid matrix, allowlist, non-goals, cross-effects, validation, and no-VM rationale before cleanup discovery. Evidence: implementation/security review passed. Evidence review required separation of directly validated strings from representable typed object paths, fresh secret-bearing zero-publication fixtures, exact 128-byte alias and collection-target ACK cases, explicit docs evidence, and precise malformed-before-lock versus dangling-under-lock ordering. The plan incorporated every correction and fresh final re-review passed on 2026-08-30 with implementation evidence pending.
- [x] Complete finding-local and repository-wide cleanup discovery; independently process accepted cleanup before SetAlias correction.
  - Finding-local review found SetAlias parsing, validator sharing, and all current publication/isolation tests live; no independent net deletion exists.
  - Shared segment extraction, early validation, handler matrices, and docs are the F-045 fix.
  - Repository-wide bounded discovery found no high-confidence private dead production deletion independent of current or later findings.
  - F-046 property typing, broad Secret Service refactors, tests/docs deletion, dependencies, formatting, and cosmetic churn are rejected.
  - Cleanup is exhausted with no accepted unit and no cleanup commit; the approved SetAlias correction may begin.
- [x] Implement and independently review early SetAlias validation and complete no-publication/valid-publication evidence without changing F-008/F-012.
  - Fix commit: `a00a7e109958949a3fd96a4d177921a9b769147a` (`fix: validate SetAlias before publication`).
  - One private segment validator now serves CreateCollection names plus SetAlias alias/target segments. SetAlias validates alias first, accepts only root clear or one exact collection segment, acquires the write lock, checks target existence in that same store, and only then enters the unchanged candidate/publication/ACK path.
  - Malformed or dangling requests return fixed value-free `InvalidArgs`; unexpected non-root paths can no longer become alias removal. Root clear, missing-alias clear, same-target set, common and 128-byte boundaries, host REJECT, wrong-generation, and reject-then-ACK behavior remain covered.
  - Fresh production review passed. Evidence review required representable typed path cases, secret-bearing fresh no-publication fixtures, both 128-byte ACK boundaries, direct-vs-handler documentation, and exact malformed-before-lock versus dangling-under-lock wording. After those scope corrections, both actual-diff reviews found no source/docs defect; final evidence review passed with no gap.
- [x] Validate, run controller scope/diff/security/artifact checks, commit coherently, and record F-045 completion/cross-effects before promoting F-046.
  - Exact staged diff: four approved paths, 292 insertions and 57 deletions, SHA-256 `b3add160b76e93e93dcac2c5900d61bfa6d88079ea9ead5e9fc1ea41aac00f1a`.
  - Focused/full validation passed 19 service-handler and 51 full secretsd tests, 23 secretsd-store tests, 24 protocol resource-blob tests, and 15 namespace publication tests (log SHA-256 `9d88c111abba1da39c59c7c2c8e64be26483493d859ca3e0a7521ccae36e3976`).
  - The packaged integration runner passed all 106 tests (`a6f535316ade1fda0bafb0efc30abc04a998959dfa0260c788a7ac14348e82e0`). Bounded workspace tests passed with exact Nix web artifact `/nix/store/q6j5j105c1y9ilcppwaph4y9j36wirrz-underwrap-web-dist-0.1.0` (`7276b38e2e6e64c8ccb432fd12d82a355bfbf851572d511ed74b33d89f124e86`; prerequisite `33fec8fdf10471630c29c0c98e6ebbe21fce3ccb07019bab8ff0fab74378da14`), workspace denied-warning Clippy and rustfmt passed (`d7dae192715495fa89ccbe279af3efb9792c1dae1768228e369035e68a1df673`), and Nix workspace-test passed (`ab036032ada141c165f6f33c5ad8e448afb45cde72a9ec9d51406cd10cebe7c6`).
  - Static validation proved alias→path→lock→existence→candidate ordering and documentation targets (`83111748b980be5ec89ea3fa0ebddb30d5de76db14b36606d92d2c2b229b3057`).
  - The implementation worker's first Nix compile found a test map-type mismatch and its first Clippy command omitted the required SPA bundle. Both were corrected before controller validation; all final focused, workspace, Clippy, integration, and Nix checks passed. These are corrected implementation/prerequisite attempts, not accepted product evidence.
  - Controller checks passed exact four-path scope, fixed/value-free error messages, representable path construction, fresh secret-bearing zero-publication fixtures, no CreateCollection optional-alias/store-API/F-046 change, no Nix/dependency/generated/secret artifact, `git diff --check`, checksum, removal of `result`, and clean post-commit tree.
  - F-045 is resolved. F-008, F-012, and F-039 remain resolved; U-004 is narrowed but remains unresolved; F-046 and every other finding/Opportunity/unresolved item remain unchanged. F-046 is next.
  - Final closure review passed commit identity, exact scope/stat/checksum, validation ledger, corrected-attempt classifications, historical alignment, no-VM rationale, and F-046 promotion. The controller then independently reconfirmed plan-only status, commit stat/hash, all seven retained log hashes, and `git diff --check`.

### Remaining low queue

Promote one row at a time using the same iteration protocol.
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

- [x] F-031 through F-037 are resolved or explicitly closed with current evidence.
- [x] The medium → low transition is explicitly approved and recorded.
- [ ] F-038 through F-058 are resolved or explicitly closed with current evidence.
- [ ] Every accepted cleanup and fix has independent review, required validation, controller checks, and an ordered local commit recorded here.
- [ ] Every cross-finding effect, stop decision, unavailable check, and residual risk has a durable owner.
- [ ] Accepted decisions, current mechanisms, research, future direction, and user-visible behavior have moved to their maintained documentation owners.
- [ ] No active ticket or competing plan owns any remaining obligation.
- [ ] The user accepts final program completion before this plan moves to `docs/plans/archived/`.
