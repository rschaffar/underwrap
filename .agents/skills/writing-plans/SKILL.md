---
name: writing-plans
description: Draft, execute, track, curate, and archive lean evidence-backed plans for non-trivial Underwrap work.
compatibility: Intended for the Underwrap repository and its docs/plans workflow.
---

# Underwrap writing plans

Plans are shared working memory for people, agents, and sessions.

An accurate plan is more valuable than an optimistic or artificially complete plan.

The active workflow is plan-based, and `docs/plans/` is the canonical owner for new work.

## When a plan is justified

Create or revise a plan when work needs sequencing across sessions, explicit product or architecture decisions, discovery, multiple validation boundaries, migration or release coordination, or durable handoff.

Use a small direct task without a plan when the change is obvious, local, independently verifiable, and does not need a decision record or cross-session obligation list.

Do not create a plan merely to duplicate a ticket, repeat a completed change, or document this workflow migration itself.

Do not create competing plans for the same implementation.

## Ground the plan

Read `docs/plans/README.md` before drafting, executing, tracking, curating, or archiving a plan.

Inspect every active plan that could overlap the requested behavior and record the relationship or resolve the overlap before editing.

Read `AGENTS.md`, the relevant sections of `docs/index.md`, `docs/architecture.md`, `docs/security.md`, `docs/testing.md`, and any task-oriented guide that defines the affected behavior.

Read relevant ADRs, implementation notes, research records, and roadmap entries before proposing a decision or relying on an earlier investigation.

Use the existing `.codegraph` index for structural exploration when it is present, then confirm important coverage with targeted source or test inspection.

Inspect `tickets/_review/` and `tickets/_handover/` only when accepted review findings or prior ticket decisions overlap the requested work.

Treat existing reports, handovers, and completed tickets as immutable historical evidence rather than as new plan checklists.

`tickets/_review/` is append-only for newly accepted comprehensive-review reports.

Search for active ownership in `docs/plans/` and for historical overlap in the retained ticket records before creating a plan.

State assumptions and unknowns that affect scope, security, implementation, or validation.

Ask at most one focused question when proceeding requires a risky product, architecture, dependency, binary-chain, or security decision.

Do not ask a question for a preference that can be safely recorded as an assumption and revisited by the user.

## Plan authority and ownership

A request to create or materially revise a plan authorizes the plan artifact and its scoped local commit, not implementation.

Execute a plan only after the user explicitly authorizes implementation or the plan itself contains clearly recorded execution authority granted by the user.

A plan cannot authorize a new dependency, a change to the `underwrap-exec` → `underwrap-ns` binary chain, a weaker security boundary, or a behavior change that still needs a decision.

The plan workflow replaces the ticket workflow for new work.

Existing ticket, handover, and report files are immutable historical evidence; `tickets/_review/` may only append a newly accepted comprehensive-review report.

Do not create a new ticket for work owned by a plan, and do not create both a plan and a ticket for the same implementation.

The accepted 2026 review program used one iterative plan rather than one replacement plan per finding and is complete in [`docs/plans/archived/2026-08-26_codebase-review-remediation-plan.md`](../../../docs/plans/archived/2026-08-26_codebase-review-remediation-plan.md).

D-174's transferred execution authority is exhausted, and the archived plan grants no authority for new work.

Work derived from its report, Opportunities, unresolved items, or evidence requires a newly grounded plan and any focused decision that its current scope needs.

The retained `underwrap-ticket-workflow` skill is interpretation guidance for historical records, not an executable workflow.

Use `underwrap-codebase-review` for whole-repository or explicitly comprehensive subsystem review, `reviewing-code` for ordinary bounded review, and `hardening-code-paths` for bounded implementation edge cases.

## Required plan structure

Save plans as `docs/plans/YYYY-MM-DD_<topic>-plan.md` with a concise lowercase topic.

Every plan must contain these headings:

```markdown
## Goal
## Plan
## Completion Checklist
```

Add optional sections only when they carry useful information:

```markdown
## Context
## Plan Relationship
## Architecture
## Security Invariants
## Non-Goals
## Assumptions
## Unknowns
## Risks
## Rollback / Recovery
```

Use `Plan Relationship` to state what adjacent plans or historical records own and what this plan deliberately excludes.

Use `Architecture` for component ownership, data flow, IPC, state transitions, storage, configuration, deployment, or system integration.

Use `Security Invariants` whenever work can affect a secret, identity, privilege, resource, policy, persistence, process lifetime, or security claim.

Use `Rollback / Recovery` for destructive changes, migrations, protocol or configuration changes, service changes, or any operation with an interruption window.

Keep the plan concise and write prose in present tense.

## Underwrap security invariants

Describe the protected asset, attacker, and boundary direction whenever a plan changes or relies on a security boundary.

Treat the caller and other caller-owned non-root processes as untrusted with respect to brokered secrets.

Treat the configured target as trusted with its projection but treat target-supplied protocol input, especially Secret Service input, as untrusted for robustness and storage integrity.

Keep the backend as policy and persistent brokered-resource authority, and keep runtime requests at wrapper/resource vocabulary rather than storage keys or `control/*` paths.

Preserve runtime-client mTLS identity derivation, exclusive configured trust choices, and the distinction between local Unix users and backend principals.

Treat wrapper names as selectors rather than authorization, reject unknown and reserved selectors, ignore `argv[0]`, and let the daemon choose the target after backend authorization.

Preserve effective-GID peer authentication, hardened `SO_PEERPIDFD` behavior, fixed fd 20 daemon authentication, descriptor-free fd 3 handoff, and fail-closed rejection without fallback.

Resolve target UID/GID only from root-owned `(instance, wrapper)` authority and permanently drop credentials, supplementary groups, capabilities, dumpability, and core limits as required by the existing design.

Keep environments cleared and allowlisted, keep descriptors above stderr closed or close-on-exec, and never place secret material in argv, inter-stage environment, shell commands, temporary disk paths, or logs.

Preserve namespace and private tmpfs boundaries, target/helper lifecycle cleanup, Secret Service publication validation, and the distinction between target trust and untrusted target protocol input.

Re-evaluate permission mode on open and finalize, re-check writable state for every writeback, preserve writable local projections for read-only policy, and describe the documented whole-blob last-writer-wins limitation without claiming CAS.

Use the NixOS VM test whenever the module, privileged wrapper chain, users, capabilities, namespaces, mounts, peer authentication, or target-identity boundary is affected.

Do not remove or weaken a security assertion merely to make a test pass.

## Write executable tasks

Order tasks by dependency and resolve material unknowns before tasks that rely on them.

Each checkbox names one action, one object, the expected result, and exact acceptance evidence.

Prefer a bounded discovery task when a design or implementation choice remains unknown.

Avoid vague tasks such as `improve security`, `handle edge cases`, or `finish later`.

Example:

```markdown
- [ ] Add a malformed fixed-fd handoff case in `underwrap-ns` and run `cargo test -p underwrap-ns handoff` to prove the selector is rejected before fd 3 is read.
```

Separate user-run checks from agent-safe checks and label hardware, host, or operator evidence accurately.

Do not plan repeated secret-submission attempts or operations that probe one secret across multiple targets.

End the plan with finite completion checks covering implementation, tests, documentation, review, scope, and handoff.

## Evidence and task lifecycle

Use measured facts instead of adjectives.

Record the command, test, environment, version, and relevant output summary when they affect the result.

Label evidence as `direct`, `simulated`, `unavailable`, or `user-reported`.

`direct` evidence is an observation from the requested repository or deployed test path.

`simulated` evidence comes from a mock, injected fault, fixture, isolated model, or other non-production substitute.

`unavailable` evidence identifies a required check that could not run and why.

`user-reported` evidence records an operator or user observation without claiming that an agent reproduced it.

Check a task only after its stated acceptance evidence passes.

A written implementation without its acceptance evidence is not complete.

Leave failed and unavailable checks open and state the missing condition.

Mark an inapplicable task as `- [x] Not applicable: <reason>`.

Reopen a checked task when later changes invalidate its evidence, alter its boundary, or supersede its acceptance condition.

Do not claim that unit tests establish zeroization, memory isolation, kernel behavior, target trustworthiness, filesystem durability, or browser accessibility unless the test directly observes that property.

## Execute a plan

Read the owning plan, its related ADRs and security guidance, and the current status before editing.

Confirm that the requested change is inside the plan and that no ticket or other plan owns it.

Stop and ask one focused question if implementation requires a risky unapproved product, architecture, dependency, binary-chain, or security decision.

Trace the real boundary end to end through callers, wire forms, state transitions, waits, cleanup, persistence, and downstream consumers before changing code.

Add the smallest regression test or executable check that fails before the fix when practical.

Fix the authoritative shared boundary rather than patching one caller, and scan only sibling paths with the same root cause.

Run focused checks after each coherent milestone and the full relevant validation before declaring the plan complete.

## Commit cadence

Creating or materially revising a plan authorizes a scoped local commit unless the user explicitly says not to commit.

Executing an authorized plan authorizes regular coherent local commits.

Commit completed plan tasks or bounded milestones instead of accumulating an entire plan in one working tree.

Update plan evidence with the implementation commit or an immediate documentation-only follow-up.

Before every commit, inspect `git status --short`, enumerate the exact allowlist, stage only those paths, verify `git diff --cached --name-only` matches it, inspect the complete staged diff, and run `git diff --cached --check`.

Exclude unrelated user changes and never unstage or overwrite them.

Include meaningful verification in non-trivial commit bodies.

Local commit authority never authorizes push, tag, publish, amend, rebase, squash, or any history rewrite.

The archived 2026 review-remediation plan retains its ordered cleanup, fix, approval, review, validation, stop, and transition evidence as a completed workflow handoff rather than an active work owner.

## Curate and archive

Move accepted decisions and trade-offs to `docs/adr/`.

Move maintained current mechanisms, operational details, and empirical findings to `docs/implementation-notes/`.

Move dated investigations, rejected options, and visible retractions to `docs/research/`.

Move future direction without implementation authority to `docs/roadmaps/`.

Move user-visible behavior and limitations to `README.md` or the appropriate public guide.

Keep published product documentation present tense and free of superseded alternatives or planning history.

Declare a plan complete only when every task and completion check is checked, material unknowns are resolved or explicitly accepted, risks have a disposition, and handoff work is complete.

Move a fully evidenced plan to `docs/plans/archived/` and report what durable content moved to which owner.

Do not archive a plan with missing evidence or leave the archive as the only owner of durable facts.

Do not delete a plan unless the user asks and every remaining obligation has an explicit current owner.

Unchecked boxes in an archived plan never become active work.
