---
name: underwrap-component-refine
description: Component-by-component refinement workflow for the underwrap repository. Use when asked to review, refine, or refactor a named part of the codebase and turn findings into tickets. This skill stops after a findings report and proposed ticket plan; execution happens separately through underwrap-ticket-workflow.
compatibility: Intended for the underwrap repository layout. Reads the active ticket board and writes review reports under tickets/_review/.
---

# Underwrap component refinement

Use this skill when the user asks for a **review and ticket plan** for a named
crate, module subtree, or bounded subsystem. It is not the workflow for a user
who explicitly asks to apply fixes in the same session.

This skill is read-only with respect to production code and the ticket board.
Its only project write is one report under `tickets/_review/`.

## Posture

Read `AGENTS.md` and the documentation relevant to the component before
reviewing it.

underwrap is greenfield and targets a polished first release:

- recommend the clean design rather than compatibility with an undeployed
  earlier shape;
- remove parallel or superseded paths instead of preserving migrations that
  serve no deployed user;
- keep deliberate first-release schema/protocol versioning when it identifies a
  format, detects incompatibility, or supports a security property;
- keep current-state code comments free of ticket, commit, and development
  history references;
- never trade away a security invariant for simpler code.

## Scope

A component can be:

- one crate;
- one module subtree;
- a clearly bounded cross-file seam such as peer authentication, libsecret
  projection, or admin session authentication.

Resolve the name to a concrete file list. If the request is broad, choose a
natural bounded slice and state it; ask only when multiple materially different
scopes are equally plausible. A small or single-file security-critical
component is still valid review scope.

Before reviewing:

1. run `git status --short` and note pre-existing changes;
2. read every file in scope end to end;
3. identify direct dependencies and dependents;
4. read relevant tests and current documentation;
5. scan `tickets/01_todo/` and `tickets/02_inprogress/` for overlap.

Send the resolved scope to the user in one short paragraph.

## Review categories

Use the categories that apply; do not manufacture findings to fill every one.
Each finding must cite `path:line` evidence.

1. **Correctness and invariants** — behavior, edge cases, concurrency, wire or
   persistence semantics.
2. **Security boundaries** — attacker/asset direction, privilege changes,
   authentication, descriptor ownership, secret handling, fail-closed behavior.
3. **Module boundaries** — cohesion, file size, public API shape, dependency
   direction.
4. **Naming and terminology** — ambiguous or stale names; divergence from the
   canonical vocabulary in `AGENTS.md`.
5. **Error handling** — missing context, swallowed errors, unchecked syscalls,
   accidental downgrade paths.
6. **Complexity and duplication** — control flow, unnecessary abstraction,
   repeated logic, avoidable allocation.
7. **Tests** — behavioral gaps, fixture duplication, test layout violations,
   oversized `tests.rs` files.
8. **Logging and observability** — secret leakage, missing decision context,
   misleading audit claims.
9. **Documentation and comments** — stale facts, history in current-state docs,
   ticket references, comments that merely narrate code.
10. **Greenfield cleanup** — compatibility code whose only purpose is an
    undeployed earlier shape.

For simplification findings, preserve observable behavior unless the report
explicitly labels and justifies a behavior change.

## Repository conventions

Check these inside the resolved scope:

- Production Rust files use sibling `tests.rs` modules, not inline
  `mod tests { ... }` blocks.
- Crate roots use the explicit `#[path = "main/tests.rs"]` or
  `#[path = "lib/tests.rs"]` form.
- Test-only crate-root re-exports do not bypass module privacy.
- Source/config/docs do not reference ticket numbers or commit hashes.
- Current documentation describes the intended first-release design rather
  than migrations or superseded alternatives.

A project-wide sweep is **opt-in**. Run one only when the user asks for a
repository convention audit or when an in-scope finding provides concrete
reason to suspect a systematic violation. Keep the resulting findings separate
from the named component and do not let a routine component review expand into
an unbounded repository audit.

Useful multiline searches require ripgrep's multiline mode:

```bash
rg -n -U '#\[cfg\(test\)\][[:space:]]*\n[[:space:]]*mod tests \{' crates/
rg -n -U '#\[cfg\(test\)\][[:space:]]*\n[[:space:]]*pub\(crate\) use' crates/
```

For source-history references, search explicit forms and verify every match:

```bash
rg -n -i '(ticket|story|epic)[[:space:]_:-]*[0-9]{3,4}|TODO\([0-9]{3,4}\)|FIXME\([0-9]{3,4}\)' \
  crates/ web/ nix/ docs/ examples/
```

Words such as `history`, `current`, or `old` in API/domain names are not findings
without contextual evidence.

## Findings report

Write one report to:

```text
tickets/_review/<component_slug>_<YYYY-MM-DD>.md
```

Use this structure:

```markdown
# Refinement review: <component>

Date: <YYYY-MM-DD>
Scope: <resolved files and boundary>
Working tree: <clean, or a concise note about pre-existing changes>

## Summary

- N findings: X must-fix, Y improvement.
- Existing overlapping tickets: <links or none>.
- Proposed ticket shape: <flat tasks | epic + story + tasks>.

## Findings

### F-001 — <title>

- **Severity:** must-fix | improvement
- **Category:** <category>
- **Scope:** component | project-wide sweep
- **Location:** `path:line-line`
- **Evidence:** <specific observation and relevant quote>
- **Recommendation:** <smallest coherent correction>
- **Behavior change:** no | yes — <effect and why it is appropriate>
- **Security boundary:** none | <asset, attacker, invariant>
- **Related tickets:** <links or none>

## Proposed ticket plan

### `NNNN_task_<name>.md` — <title>

- **Findings:** F-001, F-004
- **Scope:** <one paragraph>
- **Acceptance:** <observable criteria>
- **Validation:** <exact commands>
```

Every accepted finding must map to a proposed ticket or an existing active
ticket. Do not create a vague deferral bucket.

## Ticket packaging and numbering

Use flat task tickets for up to five small, independent changes. Use an epic,
one numbered story, and numbered tasks when there are six or more tasks or a
large cross-cutting change.

All story/task filenames use a free four-digit prefix in a `NNN0_` block at or
above `1000_`. Check todo, in-progress, and done directories before proposing
numbers.

Security-sensitive tickets quote the relevant invariant from `AGENTS.md` or
`docs/security.md` and include integration plus VM validation when the boundary
is exercised there.

## Stop condition

After writing the report:

1. summarize counts, highest-priority findings, proposed ticket shape, and the
   report path;
2. tell the user that an approved plan can be materialized and executed in a
   fresh session through `underwrap-ticket-workflow`;
3. stop.

Do not edit production files, move tickets, create todo tickets, or begin
implementation while running this skill.
