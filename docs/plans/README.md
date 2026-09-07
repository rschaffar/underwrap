# Plans

`docs/plans/` holds active executable plans.

A plan is shared working memory for non-trivial work that needs sequencing, cross-session continuity, explicit decisions, or evidence tracking.

Plan names use `YYYY-MM-DD_<topic>-plan.md` with a concise lowercase topic.

Every plan includes `## Goal`, `## Plan`, and `## Completion Checklist`.

Tasks use checkboxes and record the exact acceptance evidence required before they are checked.

## Active plans

There are no active plans.

## Completed plans

The completed test-fixture lifecycle hardening work is recorded in [`archived/2026-09-07_test-fixture-lifecycle-plan.md`](archived/2026-09-07_test-fixture-lifecycle-plan.md).

The completed release-readiness program is recorded in [`archived/2026-09-01_release-readiness-plan.md`](archived/2026-09-01_release-readiness-plan.md).

The completed F-031 through F-058 remediation program is recorded in [`archived/2026-08-26_codebase-review-remediation-plan.md`](archived/2026-08-26_codebase-review-remediation-plan.md).

The accepted report, handover, completed F-001 through F-030 tickets, and archived plans remain historical evidence and are not converted into new plans.

D-174's execution authority and severity protocol are exhausted, and the handover's D-201 ticket-materialization instruction remains historical rather than active authority.

## Ownership

The plan workflow replaces the ticket workflow for new work.

`docs/plans/` owns new non-trivial implementation, migration, release, investigation, and cross-cutting work.

Existing ticket, report, and handover files are immutable historical evidence.

`tickets/_review/` is append-only for newly accepted comprehensive-review reports.

Do not create both a plan and a ticket to own the same implementation.

Do not rewrite historical records to make this migration appear complete.

The current product contract remains in `README.md`, `docs/architecture.md`, `docs/security.md`, and the other task-oriented guides.

## Lifecycle

1. Ground a plan in the current documentation, security model, relevant decisions, active plans, and historical records that overlap its scope.
2. Create or revise one plan with bounded tasks, dependencies, non-goals, risks, and acceptance evidence.
3. Treat a request to create or revise a plan as authority for the document only, not for implementation.
4. Execute only after explicit implementation authority exists, and keep the plan current as work progresses.
5. Check a task only after its acceptance evidence passes, and reopen it when later work makes that evidence stale.
6. Move accepted decisions, current mechanisms, dated research, and future direction to their durable owners before archiving.
7. Move a fully evidenced plan to `archived/` only after every obligation has a current owner.

## Durable owners

| Content | Maintained owner |
|---|---|
| Accepted decisions and trade-offs | [`docs/adr/`](../adr/README.md) |
| Current mechanisms and empirical findings | [`docs/implementation-notes/`](../implementation-notes/README.md) |
| Dated investigations, rejected options, and retractions | [`docs/research/`](../research/README.md) |
| Future direction without implementation authority | [`docs/roadmaps/`](../roadmaps/README.md) |
| User-visible behavior and limitations | [`README.md`](../../README.md) and the public documentation map |
| Completed plan handoff | [`docs/plans/archived/`](archived/README.md) |

Read [the writing-plans skill](../../.agents/skills/writing-plans/SKILL.md) before drafting, executing, tracking, curating, or archiving a plan.
