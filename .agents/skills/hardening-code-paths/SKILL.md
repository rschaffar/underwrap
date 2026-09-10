---
name: hardening-code-paths
description: Confirm and fix edge cases in one explicitly authorized Underwrap flow.
---

# Hardening a code path

Bound the task to one intended flow and its authoritative shared boundary.
Do not turn it into an unrelated redesign or infer implementation authority from a proposed plan or prior review.

Read [agent guidance](../../../AGENTS.md), [Security](../../../docs/security.md), the affected topic owners, any current work owner, and [Testing](../../../docs/testing.md).
Inspect Git state and preserve unrelated changes.
Use CodeGraph before structural exploration when an index exists.
Ask before an unapproved dependency, product behavior, binary-chain, or security-boundary decision.

## Loop

1. Establish intended behavior from the user's request, current source, documentation and assertions.
2. Trace callers, framing, authority, state transitions, locks, resource ownership, cancellation, cleanup and persistence.
3. Select only reachable cases from the [edge-case checklist](references/edge-case-checklist.md).
4. Confirm a concrete violation, naming the asset, attacker and direction for security findings.
5. Add a regression that fails for that reason when practical, including a permitted control for a denied boundary.
6. Correct the smallest authoritative shared path and check its sibling callers for the same root cause.
7. Review opposite bounds, error precedence, partial initialization, retained endpoints, response loss and recovery.
8. Run focused checks and every required validation layer for the changed boundary.

Preserve all AGENTS.md security invariants and mandatory joint-review scopes.
Do not add secret-bearing diagnostics, alternate frontend persistence, identity fallback, silent posture downgrade, or a read-only mount that changes target behavior.
Keep the documented distinction between local writeability and persistence, lifecycle authorization and exec, ACK and durability, atomic visibility and transactions, practical wiping and comprehensive erasure.
Never weaken a security assertion to make a check pass.

## Completion

Use deterministic synchronization instead of sleeps when controlling concurrency.
Label source, unit, simulated, deployed, unavailable and user-reported evidence accurately.
Kernel isolation, filesystem durability, operator custody, target behavior and post-drop memory erasure require direct evidence before being claimed.
NixOS/privileged-boundary changes require the VM check; follow the remaining Rust, SPA, browser, generated-contract and integration requirements in Testing.
Make coherent inspected local commits through the applicable Git lifecycle and report actual changes, checks and unresolved limits.
No cleanup, remote operation, integration or history rewrite follows implicitly from a successful fix.
