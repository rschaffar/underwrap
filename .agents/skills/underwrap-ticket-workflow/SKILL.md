---
name: underwrap-ticket-workflow
description: Interpret immutable historical Underwrap ticket records and review-program decisions without creating or executing ticket work.
compatibility: Intended for the retained tickets/ review, handover, and completed-work records.
---

# Historical Underwrap ticket workflow

The active workflow is plan-based under [`writing-plans`](../writing-plans/SKILL.md).

This skill is interpretation guidance only.

It never authorizes creating, moving, revising, executing, or committing a ticket.

## Historical record layout

- `tickets/_review/` contains accepted review reports.
- `tickets/_handover/` contains the accepted 2026 review program's historical control state and decisions.
- `tickets/03_done/` contains completed ticket scope, approval, review, validation, and commit evidence.
- `tickets/01_todo/` and `tickets/02_inprogress/` are retained board paths, not active work owners.

Existing records are immutable.

Future comprehensive reviews may append a new report under `tickets/_review/`, but they must not overwrite an existing report.

## How to interpret a historical record

Read the linked report finding before relying on a ticket summary.

Treat a completed ticket as evidence for the exact scope, decision, implementation, validation, and commit it records.

Treat report locations and conclusions as baseline evidence that must be revalidated against the current tree.

Treat handover decisions as binding constraints when an active plan cites them, including product choices, severity gates, cleanup-first rules, independent review, validation, and stop conditions.

Do not infer authority for new work merely from a historical todo, next-action instruction, ticket approval, completion acceptance, or continuation statement.

For the accepted 2026 program, D-174's execution authority transferred only through the now-completed review-remediation plan and is exhausted.

D-201's strict privileged-configuration decision remains current through its ADR and product contract rather than through historical ticket authority.

## Completed historical review work

Use [`docs/plans/archived/2026-08-26_codebase-review-remediation-plan.md`](../../../docs/plans/archived/2026-08-26_codebase-review-remediation-plan.md) as the completed handoff for F-031 through F-058.

The archived plan is not an active owner and grants no authority for new implementation.

Use [`writing-plans`](../writing-plans/SKILL.md) to ground a new plan for approved work derived from historical evidence.

Link historical records rather than copying their large evidence bodies into a new plan.

## Future reviews

Use [`underwrap-codebase-review`](../underwrap-codebase-review/SKILL.md) for a new whole-repository or comprehensive subsystem review.

That workflow may append one independently validated report.

Plan creation or implementation remains a later, separately authorized step.
