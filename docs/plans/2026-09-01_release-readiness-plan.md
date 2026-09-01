# Release readiness plan

Status: Active.

Created: 2026-09-01.

Execution authority: On 2026-09-01 the user requested creation and immediate execution of this focused release-readiness plan.

Base revision: `6e5c6423c15c8fbb6bfcd8a45f6079b6712d4783` on branch `release/readiness` in the dedicated linked worktree `/home/robert/projects/projektbuero/underwrap-release-readiness`.

## Goal

Establish an evidence-backed first-release candidate by investigating the intermittent NixOS VM raw-blob equality failures, making the SPA function-coverage gate an explicit release decision, running the complete clean validation matrix and a disposable deployment walkthrough, and preparing a truthful handoff for independent security review.

This work prepares a candidate for review.

It does not claim that underwrap is released, independently audited, formally verified, or safe outside the documented threat model.

## Relationships

This plan promoted the completed VM obligation now owned by [`2026-09-01_vm-raw-blob-equality.md`](../research/2026-09-01_vm-raw-blob-equality.md) and continues to own the active [`spa-unit-coverage.md`](../roadmaps/spa-unit-coverage.md) obligation.

It is grounded in the accepted review report, historical handover, completed F-001 through F-030 records, and the archived F-031 through F-058 remediation plan.

Those records remain immutable evidence, and D-174's exhausted authority is not reused.

ADRs 0001 through 0007 and the current architecture, security, testing, deployment, configuration, backend, Admin UI, and secretsd-security documentation remain authoritative for their respective decisions and claims.

## Security invariants

- The protected asset in the VM investigation is the integrity of backend-managed canonical resource bytes across target execution, namespace teardown, daemon finalization, and backend persistence.
- The untrusted caller must not gain brokered resource material, and diagnostic work must not print resource blobs, fixture values, credentials, canaries, key material, or secret-bearing request bodies.
- Secret-safe diagnostics may use identifiers, section names, lengths, digests, status codes, and event ordering.
- Existing byte-for-byte equality assertions remain in force unless a separately reviewed stronger assertion replaces them.
- No retry, delay, exclusion, threshold reduction, or assertion removal may hide an unexplained failure.
- The backend remains the policy and persistence authority, fixed fd 20 and fd 3 retain their current authentication and handoff roles, and target identities remain root-owned dedicated UIDs.
- A configured target remains trusted with its own projection, while target-supplied protocol input remains untrusted for robustness and storage integrity.
- Review preparation must distinguish direct, simulated, unavailable, and user-reported evidence and must preserve every documented nonclaim.

## Scope and stop conditions

This authority covers read-only investigation, secret-safe test instrumentation, bounded fixes within the existing design, meaningful SPA tests, release-gate wiring and documentation selected by the user, validation automation, deployment-document corrections, and a security-review handoff.

Stop for focused user authority before adding a dependency, changing a public product or protocol contract, changing the `underwrap-exec → underwrap-ns` chain, weakening a security boundary, accepting a release-gate waiver, or using production credentials or hosts.

Do not create a release tag, publish an artifact, bump a version, or represent an internal review as the requested independent audit.

## Plan

### 1. Establish the baseline and investigate VM raw-blob equality

- [x] Confirm the work owner, clean base, branch, and isolation before mutation.
  Evidence: `docs/plans/` had no active plan; `git status --short --branch` was clean on `release/readiness`; the dedicated worktree and base revision are recorded above.
- [x] Revalidate the two roadmap obligations and their historical evidence against the current tree.
  Evidence: current `nix/tests/test.py`, `web/vite.config.ts`, `web/package.json`, both roadmap records, the accepted report and handover, and the archived remediation plan were inspected on 2026-09-01.
- [x] Trace every historically implicated F-007/F-009 equality assertion through target completion, namespace lifecycle, daemon terminal-message handling, admin observation, and backend load/save ordering, and record a bounded hypothesis table with exact source evidence.
  Evidence: [`2026-09-01_vm-raw-blob-equality.md`](../research/2026-09-01_vm-raw-blob-equality.md) traces namespace dispatch, daemon finalization, backend save ordering, atomic PlainBackend replacement, and test-driver observation and classifies every bounded hypothesis.
- [x] Add the smallest secret-safe observation needed to distinguish the remaining hypotheses, only if static tracing and existing logs are insufficient.
  Evidence: temporary length-and-SHA-256 observations exposed only permitted metadata and proved that the guest-side file remained identical while one driver response was short; the durable correction retains only secret-safe consistency failures.
- [x] Run bounded forced VM reproductions against one exact committed tree without retry masking.
  Evidence: the research record identifies each uninstrumented and diagnostic run, command outcome, failing assertion, log hash, and decisive metadata; reproduction stopped when the unchanged guest fingerprint and 230400-character driver shortfall isolated the observation boundary.
- [x] Correct the evidenced root cause with the smallest design-preserving change, or record the unresolved blocker and stop release-candidate closure.
  Evidence: `nix/tests/test.py::admin_get_blob` now performs bounded exact transfers with no retries, validates every chunk and the complete guest fingerprint, leaves all F-007/F-009 equality assertions unchanged, and has no product, protocol, storage, wrapper-chain, dependency, or security-boundary effect.
- [x] Validate the VM disposition without weakening the witness.
  Evidence: all original byte-for-byte assertions remain; five consecutive forced VM executions reported `ALL TESTS PASSED` on clean revision `49a0d310ad99aa0641f2f01987031f87c05aeff6`; the research record identifies each full-log hash and retained evidence limit.
- [x] Curate the investigation into its durable owner and retire or revise the VM roadmap obligation only after its remaining direction is accurate.
  Evidence: the dated research record owns the source trace, hypotheses, cause, correction, five-run evidence, and lower-layer unknown; the resolved roadmap item was removed, and no current product security claim changed.

### 2. Decide and implement the SPA function-coverage release policy

- [x] Reproduce the current SPA coverage result in the pinned frontend environment from a clean npm installation.
  Evidence: [`2026-09-01_spa-function-coverage.md`](../research/2026-09-01_spa-function-coverage.md) records clean revision `533d05c00d331c0edfe677b3225bd9c4cd64afba`, Node 24.19.0, npm 11.17.0, the clean-install log hash, all 29 files and 162 tests passing, status 1, and exact totals of 80.20-percent statements, 82.85-percent branches, 52.23-percent functions, and 81.03-percent lines.
- [x] Map uncovered API and library functions to observable contracts and identify the meaningful tests needed to reach the existing threshold.
  Evidence: the research record maps every zero-count API wrapper by module to method, URL/path/query placement, request-body forwarding, response decoding, and existing error-mapping coverage and rejects a percentage-only test for the production no-op branch.
- [ ] Present one focused release-gate decision with a recommendation to the user.
  Acceptance evidence: the user explicitly selects whether `npm run test:coverage` is a first-release gate and where that gate is enforced; any threshold reduction, exclusion expansion, or waiver is named rather than implied.
- [ ] Implement only the selected gate disposition.
  Acceptance evidence if gated: meaningful tests make all four existing 80-percent thresholds pass, no threshold is lowered, no exclusion is expanded, and the release matrix invokes the command at the selected enforcement point.
  Acceptance evidence if not gated: the user-approved rationale and future owner are documented, and configuration and release documentation no longer imply a silently passing gate.
- [ ] Curate the decision and retire or revise the SPA roadmap obligation only after its remaining direction is accurate.
  Acceptance evidence: an ADR owns the release policy if it is a lasting decision, current testing documentation owns the command, and the roadmap contains only genuinely future work.

### 3. Run the clean release matrix and deployment walkthrough

- [ ] Freeze one candidate revision after all accepted corrections and confirm a clean tracked tree before validation.
  Acceptance evidence: the revision, branch, `git status --short`, supported system, and pinned Rust, Cargo, Node, npm, Playwright, and browser-driver versions are recorded.
- [ ] Run the complete direct and aggregate validation matrix sequentially without silently replacing a failed run with a passing rerun.
  Acceptance evidence: `cargo test --workspace`, `cargo fmt -- --check`, `cargo clippy --all-targets --workspace -- -D warnings`, `./crates/integration-tests/run`, SPA unit tests and production build, the selected coverage disposition, Playwright, `npm ls --all`, `npm audit --package-lock-only`, the documented `npm outdated` review, `nix flake check --all-systems --no-build`, and `nix flake check` each have an exact result tied to the candidate revision.
- [ ] Classify every red or unavailable matrix row before proceeding.
  Acceptance evidence: each row is fixed and rerun, retained as a release blocker, or explicitly accepted by the user with direct, simulated, unavailable, or user-reported evidence clearly labelled.
- [ ] Walk through the documented one-backend, one-frontend, one-wrapper deployment in a disposable environment without modifying an operator host or using a production secret.
  Acceptance evidence: the steps in `docs/getting-started.md` are followed in order through backend bootstrap, store creation, runtime client enrollment, wrapper registration, value insertion, permission unlock, wrapper execution, and deployment checks; browser-equivalent API substitutions are labelled simulated and configured Playwright evidence remains separate.
- [ ] Reconcile the walkthrough with the separate-host deployment guide without claiming a multi-host test.
  Acceptance evidence: TLS, service identity, filesystem mode, enrollment, restart, permission, log-hygiene, and encrypted-store restart instructions are checked, while DNS, firewall, routing, latency, partitions, and distinct-host compromise remain explicit untested limits.
- [ ] Correct any command, ordering, or expectation that the walkthrough disproves.
  Acceptance evidence: documentation and examples describe only the verified first-release flow, and their relevant parse, evaluation, build, browser, and VM checks pass.

### 4. Prepare the independent security-review handoff

- [ ] Inventory the candidate's protected assets, attackers, trust assumptions, privileged boundaries, wire and storage formats, review hotspots, accepted ADRs, and known limitations without duplicating normative prose.
  Acceptance evidence: every handoff statement links to its current owner in architecture, security, secretsd-security, backend, testing, source, or an ADR.
- [ ] Produce a concise reviewer guide for the exact candidate.
  Acceptance evidence: the guide states review scope and non-scope, supported platforms and toolchains, source/build entry points, threat model, high-value call paths, validation commands, dependency and generated-contract inputs, known blockers and nonclaims, and how evidence was classified.
- [ ] Produce a dated release-readiness evidence record.
  Acceptance evidence: it identifies the exact revision and environment, records every matrix and walkthrough result, distinguishes fresh evidence from historical evidence, and contains no credential, secret, resource blob, key, canary, or secret-bearing fixture content.
- [ ] Check current documentation and generated contracts for review-blocking contradictions.
  Acceptance evidence: OpenAPI drift, documentation links and paths, example evaluation, terminology, release posture, audit status, and known-limit statements are reviewed and any contradiction is corrected or listed as a blocker.
- [ ] Obtain a bounded independent read-only review of the handoff, evidence record, and all release-readiness changes.
  Acceptance evidence: fresh review reports no unresolved correctness, security-boundary, evidence, scope, or overclaim finding, or accepted corrections are applied and re-reviewed.

### 5. Curate and close

- [ ] Move every lasting decision, current mechanism, dated investigation, limitation, and future obligation to its designated durable owner.
  Acceptance evidence: ADRs, implementation notes, research, roadmaps, public documentation, and source each own only the material defined by the project-memory taxonomy.
- [ ] Complete final branch and artifact hygiene.
  Acceptance evidence: the candidate tree is clean; generated local schema, coverage output, Playwright artifacts, result links, logs, processes, and temporary secret-bearing material are absent or safely outside the repository; committed files contain no secret values.
- [ ] Present the complete candidate disposition to the user before archival or integration.
  Acceptance evidence: the user receives exact commits, matrix results, walkthrough classification, security-review handoff location, blockers and known gaps, and chooses whether the plan may be archived and the branch integrated.

## Risks and unknowns

- Repeated full VM runs are expensive, and a bounded pass streak cannot prove the absence of a race.
- Secret-safe instrumentation can still perturb scheduling, so static ordering evidence and uninstrumented confirmation runs remain necessary.
- The local disposable walkthrough cannot establish the separate-host network properties explicitly excluded by current documentation.
- The SPA coverage decision is a release-policy choice and remains unresolved until the user answers the focused gate.
- Independent security review may identify new work, and this plan must not pre-accept or silently absorb that work.

## Completion Checklist

- [ ] The intermittent raw-blob equality obligation has an evidenced cause and validated correction, or release readiness is explicitly blocked rather than waived implicitly.
- [ ] The SPA coverage release policy is user-selected, durably owned, internally consistent, and implemented.
- [ ] Every row of the clean release matrix has a fresh candidate-revision result and no unexplained red outcome.
- [ ] The disposable deployment walkthrough completes with evidence classifications and corrected documentation.
- [ ] The independent security-review guide and dated evidence record are complete, truthful, secret-free, and independently reviewed.
- [ ] No dependency, binary-chain, product-contract, protocol, or security-boundary change crossed its stop condition without focused authority.
- [ ] The repository remains explicit that underwrap is pre-release and has not received an independent end-to-end security audit.
- [ ] Durable curation, final user acceptance, archival, branch integration, and worktree cleanup are complete.
