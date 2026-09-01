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

This plan promoted and completed the VM and SPA roadmap obligations now owned by [`2026-09-01_vm-raw-blob-equality.md`](../research/2026-09-01_vm-raw-blob-equality.md), [`2026-09-01_spa-function-coverage.md`](../research/2026-09-01_spa-function-coverage.md), and [`ADR-0008`](../adr/0008_spa-unit-coverage-gate.md).

It is grounded in the accepted review report, historical handover, completed F-001 through F-030 records, and the archived F-031 through F-058 remediation plan.

Those records remain immutable evidence, and D-174's exhausted authority is not reused.

ADRs 0001 through 0008 and the current architecture, security, testing, deployment, configuration, backend, Admin UI, and secretsd-security documentation remain authoritative for their respective decisions and claims.

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
- [x] Present one focused release-gate decision with a recommendation to the user.
  Evidence: after receiving the fresh totals, mapped contracts, enforcement options, recommendation, and explicit trade-off explanation, the user selected and reconfirmed the existing Nix `web-test` check as the first-release enforcement point.
- [x] Implement only the selected gate disposition.
  Evidence: 30 endpoint-contract cases make all 30 files and 192 tests pass at 96.35-percent statements, 82.85-percent branches, 98.50-percent functions, and 98.27-percent lines; `web-test` runs `test:coverage`; no threshold or exclusion changed; direct coverage, production build, and the Nix derivation passed.
- [x] Curate the decision and retire or revise the SPA roadmap obligation only after its remaining direction is accurate.
  Evidence: [`ADR-0008`](../adr/0008_spa-unit-coverage-gate.md) owns the accepted policy and nonclaims, `docs/testing.md` owns direct and aggregate commands, the dated research record owns the investigation and evidence, and the completed roadmap item was removed.

### 3. Run the clean release matrix and deployment walkthrough

- [x] Freeze one candidate revision after all accepted corrections and confirm a clean tracked tree before validation.
  Evidence: clean revision `f61d8ddeb76de76dad0e4c8bffc45dcee7c0bd9e` on `release/readiness`, x86-64 Linux 6.18.48, Rust/Cargo 1.97.1, Node 24.19.0, npm 11.17.0, Playwright 1.61.1, driver Node 22.23.2, and Chromium 149.0.7827.55 are recorded in the dated evidence.
- [x] Run the complete direct and aggregate validation matrix sequentially without silently replacing a failed run with a passing rerun.
  Evidence: [`2026-09-01_release-readiness-evidence.md`](../research/2026-09-01_release-readiness-evidence.md) records every required command, exact result, local log hash, threshold total, and key Nix output for the clean candidate; all 65 current-system flake checks passed.
- [x] Classify every red or unavailable matrix row before proceeding.
  Evidence: [`2026-09-01_workspace-validation-stability.md`](../research/2026-09-01_workspace-validation-stability.md) owns every corrected pre-candidate failure; the evidence record classifies the expected `npm outdated` status, one auxiliary parser error, AArch64 evaluation-only evidence, and unavailable separate-host properties.
- [x] Walk through the documented one-backend, one-frontend, one-wrapper deployment in a disposable environment without modifying an operator host or using a production secret.
  Evidence: a purpose-built NixOS VM completed offline bootstrap, plain-store creation, keygen and enrollment, auto-registration, env insertion, permission unlock, both wrapper forms, deployment checks, and backend restart; the successful full-log hash is `534d06c67c24f3a3c26b8b8805a952f8ad43fe2556fb9e47a3c8d16fe94392c6`.
- [x] Reconcile the walkthrough with the separate-host deployment guide without claiming a multi-host test.
  Evidence: the dated evidence distinguishes direct same-host TLS, service-identity, mode, enrollment, restart, permission, log-hygiene, and encrypted-restart evidence from unavailable DNS, firewall, routing, latency, partition, orchestration, and distinct-host evidence.
- [x] Correct any command, ordering, or expectation that the walkthrough disproves.
  Evidence: three temporary harness-only assumptions were corrected and classified; the documented sequence and product contracts held, no product guide correction was required, and the candidate examples, browser suite, generated contract tests, and VM passed.

### 4. Prepare the independent security-review handoff

- [x] Inventory the candidate's protected assets, attackers, trust assumptions, privileged boundaries, wire and storage formats, review hotspots, accepted ADRs, and known limitations without duplicating normative prose.
  Evidence: [`2026-09-01_security-review-handoff.md`](../research/2026-09-01_security-review-handoff.md) links each summary to the current architecture, security, secretsd-security, backend, configuration, testing, ADR, source, or evidence owner.
- [x] Produce a concise reviewer guide for the exact candidate.
  Evidence: the handoff names candidate `f61d8dd`, the complete release range, supported scope, threat-model entry points, six high-value call-path groups, build and generated inputs, review questions, unavailable properties, and nonclaims.
- [x] Produce a dated release-readiness evidence record.
  Evidence: the evidence record identifies the candidate and environment, records direct and simulated matrix and walkthrough evidence, labels unavailable and historical limits, and contains no credential, resource value, blob, key, canary, or secret-bearing fixture content.
- [x] Check current documentation and generated contracts for review-blocking contradictions.
  Evidence: OpenAPI export and generated SPA bindings passed; all 247 relative documentation paths and targeted anchors resolved; flake example evaluations passed; terminology and release/audit posture inventory found no blocking contradiction; bounded new-prose and credential-pattern checks passed.
- [ ] Obtain a bounded independent read-only review of the handoff, evidence record, and all release-readiness changes.
  Unavailable evidence: the delegated reviewer failed before startup because the installed Pi bundle lacked `dist/bundle/cli.js`; it produced no review result, so independent acceptance remains pending rather than inferred.

### 5. Curate and close

- [x] Move every lasting decision, current mechanism, dated investigation, limitation, and future obligation to its designated durable owner.
  Evidence: ADR-0008 owns the selected gate, source and tests own the corrections, three dated investigations own evidence and residual observations, current product guides remain normative, and no completed roadmap or historical ticket was revived.
- [x] Complete final branch and artifact hygiene.
  Evidence: generated schema, SPA build and coverage output, Playwright results, `node_modules`, candidate-prefixed temporary logs/scripts/profiles, and diagnostic merge files were removed; no result link or candidate-owned process remained; bounded credential-pattern and diff checks passed; the tracked tree was clean.
- [ ] Present the complete candidate disposition to the user before archival or integration.
  Acceptance evidence: the user receives exact commits, matrix results, walkthrough classification, security-review handoff location, blockers and known gaps, and chooses whether the plan may be archived and the branch integrated.

## Risks and unknowns

- Repeated full VM runs are expensive, and a bounded pass streak cannot prove the absence of a race.
- Secret-safe instrumentation can still perturb scheduling, so static ordering evidence and uninstrumented confirmation runs remain necessary.
- The local disposable walkthrough cannot establish the separate-host network properties explicitly excluded by current documentation.
- Coverage instrumentation can shift during a pinned frontend-tool refresh, so changed totals require behavioral review rather than mechanical percentage work.
- Independent security review may identify new work, and this plan must not pre-accept or silently absorb that work.

## Completion Checklist

- [x] The intermittent raw-blob equality obligation has an evidenced cause and validated correction, or release readiness is explicitly blocked rather than waived implicitly.
- [x] The SPA coverage release policy is user-selected, durably owned, internally consistent, and implemented.
- [x] Every row of the clean release matrix has a fresh candidate-revision result and no unexplained red outcome.
- [x] The disposable deployment walkthrough completes with evidence classifications and corrected documentation.
- [ ] The independent security-review guide and dated evidence record are complete, truthful, secret-free, and independently reviewed.
- [x] No dependency, binary-chain, product-contract, protocol, or security-boundary change crossed its stop condition without focused authority.
- [x] The repository remains explicit that underwrap is pre-release and has not received an independent end-to-end security audit.
- [ ] Durable curation, final user acceptance, archival, branch integration, and worktree cleanup are complete.
