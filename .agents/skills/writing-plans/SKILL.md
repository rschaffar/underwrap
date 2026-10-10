---
name: writing-plans
description: Keep a bounded scope and evidence checklist when non-trivial work needs continuity.
---

# Planning work

Use a plan when sequencing, decisions, several validation boundaries, or cross-session continuity make it useful.
An obvious local change does not need one.

Read [agent guidance](../../../AGENTS.md), [contributing](../../../CONTRIBUTING.md), the relevant current topic owners, and any existing work owner before drafting.
Do not create competing issue, PR, or plan ownership for the same change.
Use CodeGraph before structural code exploration when an index exists.

## Scope and authority

State the goal, intended behavior, non-goals, material assumptions, and the actual execution authority.
A request for a plan authorizes planning only, not implementation.
Ask a focused question before an unapproved dependency, privileged-chain, product, or security decision.
Name the protected asset, attacker, and boundary direction when security is involved.

Use a short checkbox list in the chosen work owner or an external working note.
Each item names an action, expected result, and the evidence needed to mark it complete.
Include recovery or interruption handling when the operation can destroy or strand state.
A general committed planning directory or completion archive is not required.
For work arising from a review, follow the [review-findings convention](../../../docs/review-findings/README.md): suggestions and acceptance criteria are not an approved plan.
A small explicitly authorized fix may keep its scope/evidence checklist in `finding.md`; a larger or shared correction links one agreed plan from all affected findings.
Do not create competing plans or treat a shared fix as automatic acceptance of every linked finding.

## Evidence and completion

Check an item only after its evidence passes; label simulation, unavailable checks, and user-reported results accurately.
Reopen it when changed code or a changed boundary invalidates the evidence.
Keep meaningful unresolved obligations attached to an explicit current owner rather than silently calling them complete.
Follow the relevant [testing requirements](../../../docs/testing.md); never weaken an assertion or claim broader protection just to close an item.

When work completes, put current behavior and limitations in their topic guide, important rationale beside the mechanism, and regression evidence in tests.
Update the linked finding's decision summary and resolution evidence; archive it only under the convention's explicit disposition and verification rules.
A partial correction or missing required evidence remains unresolved, and accepting risk requires a maintainer decision.
Do not copy execution transcripts, approval chains, temporary hashes, or a completion diary into product documentation.
Temporary notes may be discarded once no durable fact or obligation depends on them, subject to the user's instructions and normal file/Git safeguards.
Implementation, integration, publication, history rewriting, and worktree cleanup retain their separate authority boundaries.
