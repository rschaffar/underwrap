# ADR-0001: Use plans as the active work workflow

Status: Accepted

Date: 2026-08-26

## Decision

`docs/plans/` is the canonical active workflow for new Underwrap work.

Create a plan for non-trivial implementation, migration, release, investigation, or cross-cutting work that needs sequencing, decisions, or durable evidence.

A plan request authorizes the plan artifact only, while implementation requires separate explicit authority or an execution-authorized plan.

`tickets/` remains in the repository as an immutable historical record of the accepted review program, its reports, its handover, and its completed ticket work.

Completed tickets are not copied into plans, and the existing report and handover are not rewritten as part of this migration.

Outstanding review findings may be addressed by a newly grounded plan that cites the report and handover and carries forward their applicable severity, approval, independent-review, cleanup, fix, validation, and evidence constraints.

No new ticket is created for work that belongs to the plan workflow.

The moved `underwrap-ticket-workflow` skill remains available as a compatibility reference for historical ticket records and is not the default workflow for new work.

Whole-repository and explicitly comprehensive subsystem reviews continue to use `underwrap-codebase-review`.

Ordinary bounded diff and commit reviews use `reviewing-code`, and bounded edge-case implementation uses `hardening-code-paths`.

## Alternatives considered

Keeping tickets as the active workflow would preserve a board that no longer matches the desired cross-session planning model.

Duplicating tickets and plans would create competing owners for implementation obligations and make completion evidence ambiguous.

Moving or rewriting completed tickets would discard useful historical evidence and alter accepted review records without need.

## Consequences

Active plans provide one visible owner for new obligations, dependencies, evidence, and archival decisions.

Historical tickets remain searchable evidence without continuing to create board state for new work.

Migrated review remediation must preserve the security model that treats callers as untrusted with respect to brokered secrets, configured targets as trusted with untrusted protocol input, and the backend as policy authority.

A plan must retain relevant Underwrap boundaries such as runtime-client mTLS identity, wrapper selection, fixed fd 20 authentication, fd 3 handoff, dedicated target UIDs, permanent privilege drops, descriptor and environment cleanup, writable projection semantics, and backend writeback revalidation.

The replacement does not authorize dependency changes, binary-chain changes, weakened security boundaries, or product behavior changes without their own explicit decision.

The handover's D-201 next-action instruction to materialize F-031 as a ticket is historical and is superseded by this decision.

D-201's approved strict configuration grammar remains an input constraint for any future F-031 plan.
