---
name: hardening-code-paths
description: Confirm and fix bounded edge cases and failure modes in one specific Underwrap flow.
compatibility: Intended for Underwrap code paths with an owning plan, focused tests, and the repository validation matrix.
---

# Underwrap hardening code paths

Inspect the requested flow, confirm the intended behavior, and fix confirmed edge cases without turning a bounded task into a broad redesign.

Use this skill for implementation work in one specific Underwrap flow.

Read the owning plan before editing.

If an already-started historical ticket is referenced, read its ticket, report, and handover constraints, but do not create a replacement ticket or duplicate completed work.

Do not execute work merely because a plan or ticket exists when implementation authority is not explicitly recorded.

## Bound the work

Read `git status --short` and `git diff --cached --name-only` before starting when the request does not provide an exact scope.

Preserve unrelated working-tree and staged changes.

Read `docs/plans/README.md`, the owning plan, relevant ADRs, `docs/architecture.md`, `docs/security.md`, `docs/testing.md`, and the directly affected source and tests.

Use the existing `.codegraph` index for structural exploration when it is present, then confirm important callers and coverage with targeted inspection.

State material assumptions and unknowns.

Ask exactly one focused question when the fix requires an unapproved product, architecture, dependency, binary-chain, or security-boundary decision.

Stop and create or revise a focused plan when the issue cannot be fixed within one authoritative flow without such a decision.

## Hardening loop

1. Confirm the intended behavior from the owning plan, current documentation, tests, configuration, and surrounding implementation.
2. Trace the real boundary end to end through callers, protocol forms, authorization, state transitions, waits, cancellation, cleanup, persistence, and downstream effects.
3. Read `references/edge-case-checklist.md` and select only domains reachable from this flow.
4. Build a bounded risk matrix that prioritizes secret exposure, cross-identity access, privilege errors, policy bypass, hangs, data loss, orphaned processes, and broken core behavior.
5. Establish that each candidate violates intended behavior before editing.
6. Add the smallest regression test or executable check that fails before the fix when practical.
7. Fix the authoritative shared boundary rather than patching one caller.
8. Scan only sibling paths with the same root cause and stop when that class is covered.
9. Review the diff for opposite bounds, stale state after awaits, cancellation, cleanup, fallback, error precedence, and fail-closed behavior.
10. Run focused checks, then the full relevant validation gate before reporting completion.

Do not change behavior on an unconfirmed risk.

Do not silently broaden the flow to a whole-repository audit.

## Underwrap boundaries to preserve

Treat the caller and caller-owned non-root processes as untrusted with respect to brokered secrets.

Treat the configured target as trusted with the projection but treat its protocol input, especially Secret Service input, as untrusted for robustness and storage integrity.

Keep the backend as policy and persistent brokered-resource authority.

Keep runtime requests at wrapper/resource vocabulary and do not introduce storage-key or `control/*` selectors.

Preserve runtime-client mTLS identity, configured CA semantics, HTTPS-only transport, and the distinction between backend principals and local Unix users.

Treat the wrapper selector as data, reject unknown and reserved names, ignore `argv[0]`, and let the daemon choose the target after authorization.

Preserve effective-GID socket authentication, hardened `SO_PEERPIDFD` selection, fixed fd 20 authentication, descriptor-free fd 3 handoff, and no fallback after an authenticated-path failure.

Resolve target identity only from the root-owned `(instance, wrapper)` table and preserve permanent ID, group, capability, dumpability, and core-limit transitions.

Keep environments cleared and allowlisted and close or mark close-on-exec every descriptor above stderr before target execution.

Never place secret material in argv, inter-stage environment, shell commands, ordinary temporary files, or logs.

Preserve private PID, mount, IPC, and UTS namespaces, private tmpfs staging, idmapped access-path rules, target/helper lifecycle cleanup, and target-owned runtime boundaries.

Preserve Secret Service publication ownership, readiness synchronization, canonical generation validation, ACK semantics, size limits, and target-facing malformed-input rejection.

Re-evaluate permission mode on open and finalize and re-check the writable flag on every submitted writeback.

Keep read-only projections writable locally and discard persistence rather than changing target behavior through a mount fallback.

Preserve the documented whole-blob last-writer-wins behavior unless a separately approved design changes it.

Zeroize secret buffers where practical and do not add secret-bearing diagnostics.

Security mechanisms fail closed, and a test must never be made green by removing or weakening an assertion.

## Tests and validation

Test the denied path and a permitted control path for a security boundary when both are reachable.

Control concurrency and cancellation ordering with deterministic synchronization rather than sleeps when practical.

Test empty, malformed, maximum-size, truncated, duplicated, wrong-typed, and retained-endpoint cases that can reach the flow.

Test partial initialization, response loss, timeout, cancellation, child cleanup, persistence failure, and retry behavior when they can change the outcome.

Label fixture, injected, simulated, unavailable, and user-reported evidence accurately.

Do not claim a unit or fixture test proves kernel isolation, allocator zeroization, target trustworthiness, filesystem durability, or operator behavior unless it directly observes that property.

Require the VM test when the NixOS module, privileged wrapper chain, users, groups, capabilities, namespaces, mounts, peer authentication, target UID authority, or deployed service topology is affected.

Run the focused crate or SPA tests, integration tests, formatting, Clippy, OpenAPI checks, Nix evaluation, browser checks, or VM check required by the changed boundary.

For a document-plan-owned change, commit coherent milestones regularly and update the plan evidence with the implementation commit or an immediate documentation-only follow-up.

For work constrained by historical accepted-review records, carry forward their stricter approval, independent review, cleanup/fix, handover, and validation conditions instead of bypassing them.

## Commit and handoff

Before each local commit, inspect status and staged paths, stage only the exact scoped allowlist, inspect the complete staged diff, run `git diff --cached --check`, and include meaningful verification in the commit body.

Local commit authority never authorizes push, tag, publish, amend, rebase, squash, or history rewriting.

Report the confirmed case, shared root cause, files changed, tests run, and evidence limits.

State any unavailable host, browser, VM, operator, or hardware evidence explicitly.

Do not claim protection beyond the direct evidence.
