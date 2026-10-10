---
name: reviewing-code
description: Perform a bounded read-only review of a diff, commit, pull request, or source change.
---

# Reviewing code

Review the requested scope rather than silently expanding into a whole-repository audit.
Source review is read-only unless the user separately authorizes fixes.
Record actionable findings under the [project convention](../../../docs/review-findings/README.md), subject to its no-write and confidentiality exceptions and the Git lifecycle.
Creating a finding record does not authorize implementation work.

## Method

1. Establish the exact paths, working-tree state, and requested range; use the actual merge base for branch comparisons.
2. Read [agent guidance](../../../AGENTS.md), [Security](../../../docs/security.md), the relevant topic owners, and any current work owner.
3. Use CodeGraph first for structural exploration when an index exists, then read the changed source and important assertions completely.
4. Trace the affected flow through callers, wire forms, identity, state transitions, failures, cancellation, ownership, persistence, and downstream consumers.
5. Apply the mandatory joint-review scopes in AGENTS.md and [validation matrix](../../../docs/testing.md).
6. Verify candidate findings with direct evidence and focused checks when feasible; distinguish inspected tests from tests actually run.
7. Report confirmed findings first, retain meaningful verification gaps, and stop at the requested boundary.

## Questions to test

- Does the implementation preserve caller/target trust, backend policy authority, selected identity and transport, fixed-fd authentication, and fail-closed behavior?
- Do empty, malformed, oversized, truncated, duplicated, retained-endpoint, stale-state, cancellation, and response-loss cases preserve the documented outcome?
- Are secret copies, diagnostics, environment values, temporary paths, descriptors, credentials and buffers owned and cleaned up appropriately?
- Do unsafe blocks explain pointer, lifetime, alignment, fd ownership and checked syscall invariants?
- Are resource mutations, registry publication, writeback and key lifetime described without inventing transactions, immediate erasure, or stronger durability?
- Do OpenAPI, generated clients, UI behavior, Nix options, examples and required test layers agree with the changed contract?
- Do assertions exercise the actual boundary, including denied and permitted controls, rather than a vacuous sink or incidental source shape?
- Does new prose preserve current limitations and rationale without duplicate ownership or historical references?

Do not report preference-only style comments or speculative redesigns as defects.
Do not replace meaningful evidence with test names, historical approval, or a successful aggregate command.

## Report

For each confirmed finding include priority, concrete trigger, path and line, evidence, consequence, confidence, smallest correction, and checks run or unavailable.
For security findings also name the asset, attacker, direction, and violated invariant.
Use critical for demonstrated severe security/integrity failures or inability to use the supported product, high for material blocking defects, medium for meaningful bounded corrections, and low for localized polish.
If no finding survives verification, say so and state meaningful remaining limits.
Create or update the finding's brief `README.md`, technical `finding.md` and bounded `evidence.md` under the project convention; preserve stable IDs, aliases, uncertainty and existing work ownership.
Keep raw reports and intermediate evidence private and external; essential evidence in the record must not depend on temporary artifacts.
A no-write review reports externally without importing records.
Give a mergeability verdict only when requested; a review does not authorize integration or publication.
