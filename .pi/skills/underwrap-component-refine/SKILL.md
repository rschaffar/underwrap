---
name: underwrap-component-refine
description: Component-by-component refinement workflow for the underwrap repository. Use when asked to review, refine, or refactor a named part of the codebase (a crate, a module subtree, or a subsystem) and turn findings into tickets. The skill stops after producing a findings report and a proposed ticket plan; execution happens in a separate session via underwrap-ticket-workflow.
compatibility: Intended for the underwrap repository layout. Reads tickets/ to avoid duplicates and writes the findings report under tickets/_review/. Pairs with the underwrap-ticket-workflow skill for the execution phase.
---

# Underwrap Component Refinement

Use this skill when the user names a part of the codebase and asks you to
review it, find what should be cleaned up, and turn the findings into
tickets that a later session will execute.

The skill is **review-and-propose only**. It never edits production code
and never moves tickets. Its output is a findings report and a proposed
ticket plan written under `tickets/_review/`. Execution is a separate
session that picks the tickets up via `underwrap-ticket-workflow`.

## Inputs

The user names a **component**. A component is one of:

- a single crate (e.g. `crates/frontend-exec`)
- a module subtree inside a crate (e.g.
  `crates/frontend-underwrapd/src/wrapper_flow/`)
- a named subsystem that spans a few files but is clearly bounded (e.g.
  "the libsecret channel", "the admin auth gate")
- one of the named architectural seams from `CLAUDE.md`

If the user names something larger than that (e.g. "the daemon", "the
backend"), pick the natural sub-component you'd start with, state it,
and ask the user to confirm before proceeding.

## Phases

### 1. Scope

- Resolve the component to a concrete file list and read every file in
  scope at least once. Read `CLAUDE.md` and any nearby `docs/*.md` that
  the component is referenced from.
- List the **dependents** (who imports from this component) and the
  **dependencies** (what this component imports). This is needed to
  judge API-surface findings.
- Skim `tickets/01_todo/` and `tickets/02_inprogress/` for tickets that
  already cover the same area. Tickets that overlap with a finding are
  cross-referenced in the report instead of producing a duplicate.

State the resolved scope back to the user in one short paragraph before
starting the review.

### 2. Review

Walk the files and check each of the categories below. Findings must
cite `file_path:line_number` and quote enough of the code to be
actionable without re-reading. Do not edit code in this phase.

**Default categories (all on unless the user narrows the call):**

1. **Module boundaries & file size** — single reason to touch, files
   over ~600 lines, mixed concerns in one module.
2. **Naming & terminology** — stale names, leftover terms from earlier
   designs, inconsistent vocabulary across modules.
3. **Error handling** — `anyhow` context quality, swallowed errors,
   missed return-value checks, fallback modes that should abort.
4. **Dead code & API hygiene** — unused items, over-broad `pub`, leaky
   re-exports, `#[cfg(test)] pub(crate) use` shims at the crate root
   (forbidden by `CLAUDE.md`).
5. **Test layout & coverage** — sibling-file `tests.rs` pattern
   compliance, `test_support.rs` usage, behavioural coverage gaps,
   oversized `tests.rs` files (>600 lines).
6. **Complexity** — long functions, deep nesting, gnarly match arms,
   control flow that's hard to read at a glance.
7. **Duplication / missed reuse** — repeated patterns that should be
   helpers, copy-paste between modules, re-implemented stdlib.
7a. **Simplification (same behaviour)** — places where the code does
    more than it needs to without changing observable behaviour.
    Look specifically for:
    - needless abstractions or wrapper types with a single caller
    - defensive branches that can't fire (trust internal code,
      validate only at system boundaries — the global rule from
      `CLAUDE.md`)
    - redundant clones, allocations, or `to_owned()` calls
    - layers of indirection that exist for hypothetical future
      requirements rather than current callers
    - hand-rolled logic where a short stdlib / `itertools` /
      iterator-combinator call would do
    - options, configs, or feature flags that are never set to
      anything but the default
    - error-handling code paths for impossible states
    - re-implementations of patterns already present elsewhere in the
      crate (cross-check against the "Duplication / missed reuse"
      category)
8. **Logging & observability** — secret-material leakage risk, missing
   structured fields, log-level hygiene, missing audit events.
9. **Greenfield cleanups** — compat shims, parallel-with-legacy code,
   awkward early design choices that `CLAUDE.md` explicitly calls
   replaceable.
10. **Doc & comment hygiene** — missing rustdoc on `pub` items,
    WHAT-comments that should be deleted (per the global comment rule),
    stale references to renamed things.

### 3. Findings & triage

Produce a single report at
`tickets/_review/<component_slug>_<YYYY-MM-DD>.md`. Use this structure:

```markdown
# Refinement review: <component>

Date: <YYYY-MM-DD>
Reviewer session: <model id>
Scope: <one short paragraph from phase 1>

## Summary

- N findings across M categories.
- K must-fix, L nice-to-have, P out-of-scope/needs-discussion.
- Proposed ticket shape: <epic + tasks | flat tasks> (see "Ticket plan" below).

## Findings

### F-001 — <short title>

- **Category:** <one of the ten>
- **Severity:** must-fix | nice-to-have | out-of-scope
- **Location:** path/to/file.rs:NN-NN
- **Observation:** <what is there now, quoting the relevant lines>
- **Recommendation:** <concrete change>
- **Behavior change?** no | yes — <why it's still safe / why it routes elsewhere>
- **Related tickets:** <links to existing tickets, if any>

[... F-002, F-003, ...]

## Out-of-scope / routed elsewhere

Findings that fall outside the refactor scope rule (see "Scope discipline"
below). For each, name the destination: a separate non-refactor ticket,
the security-review skill, a wire-format/NixOS-module decision needing
user sign-off, etc.

## Ticket plan

<one of the two shapes from "Ticket packaging" below>
```

### 4. Proposed ticket plan

Do **not** write ticket files in `tickets/01_todo/`. Instead, include a
"Ticket plan" section in the findings report that fully specifies each
ticket so a follow-up session can create them mechanically:

- proposed filename (with numeric prefix, see "Numbering" below)
- title
- one-paragraph description
- list of finding IDs (`F-007, F-012`) the ticket resolves
- the acceptance check (which `cargo test` / `cargo clippy` /
  `nix build` invocation must stay green)

End the report with one sentence telling the user how to proceed:
"Approve the plan and start a fresh session — it will pick this report
up from `tickets/_review/` and create the tickets, then execute them via
`underwrap-ticket-workflow`."

### 5. Stop

After writing the report, summarise it in chat (counts per severity,
counts per category, the proposed ticket shape) and stop. Do not
create tickets. Do not edit code. Do not start a new component.

## Ticket packaging

Pick per component:

- **Epic + numbered tasks** when there are **6 or more** proposed task
  tickets, or when at least one task is non-trivial (>~200 LoC of
  movement). Mirror the structure of
  `tickets/01_todo/epic_backend_modularization.md`: one
  `epic_<component>_refinement.md`, one
  `NNN_story_<component>_refinement.md`, and the `NNN_task_*.md`
  children under it.
- **Flat task tickets only** when there are 5 or fewer, all small and
  independent. Use just `NNN_task_<short_title>.md` files.

State which shape you chose and why in the report's summary.

### Numbering

New tickets use a **4-digit** zero-padded prefix (`NNNN_`). Pick a free
`NNN0_` block at or above `1000_` that doesn't collide with anything in
`tickets/01_todo/`, `tickets/02_inprogress/`, or `tickets/03_done/`.
Tasks within an epic are ordered so that later tasks build on earlier
ones (same convention as the existing epics).

Older tickets still use 3-digit prefixes (`000_`–`820_`); do not reuse
those even if a slot looks free, and do not rename them. The 4-digit
convention only applies to new tickets.

## Scope discipline

Refactor tickets emitted by this skill may include:

- pure refactors: moves, renames, file splits, dead-code removal, test
  relocation, doc cleanups
- API-hygiene tightening (`pub` → `pub(crate)`, removing leaky
  re-exports)
- **small correctness wins inline**: better `anyhow` context, missing
  `zeroize`, missed return-value checks, log-level fixes, missing
  rustdoc on `pub` items

Anything bigger is **flagged in findings but routed elsewhere**, never
silently mixed into a refactor ticket:

- Bug fixes with observable behaviour change → separate non-refactor
  task ticket, owner: user.
- Security mechanism review → routed to the `security-review` skill.
- Wire-format, NixOS module structure, or config-schema changes →
  flagged as "needs user sign-off", never auto-ticketed even though
  `CLAUDE.md` permits them in greenfield.
- Cross-component architectural rework → flagged as a candidate epic,
  owner: user.

The "Behavior change?" field on every finding makes this routing
explicit.

## Hard rules

- **Never edit production code in this skill.** Reading and grepping
  only.
- **Never move tickets or write into `tickets/01_todo/`,
  `tickets/02_inprogress/`, or `tickets/03_done/`.** This skill only
  writes under `tickets/_review/`.
- **Never propose a ticket that weakens a security boundary** described
  in `CLAUDE.md` "Security-critical rules" or in `docs/security.md`. If
  a refactor would touch one of those, route it to `security-review`
  instead.
- **Never propose `--no-verify`, `--no-gpg-sign`, or any hook skip** in
  acceptance checks.
- **One report per call.** If the user names two components, do them as
  two separate calls so each report stands alone.

## Working style

- Read first, write second. The report only goes out once every file in
  scope has been read end-to-end at least once.
- Cite line numbers. A finding without a citation is not a finding.
- Be terse in chat; the report is the artefact. The chat summary is a
  few sentences plus the counts.
- If a finding requires a judgement call the user should make (e.g.
  "rename `XyzClient` to `XyzRuntimeClient` everywhere"), mark it
  **nice-to-have** with an explicit "decision needed" note rather than
  **must-fix**.
- If the component is too small to need a review (one file, <200
  lines), say so and stop. Don't manufacture findings.

## Pairing with the execution session

The execution session reads the report from `tickets/_review/`,
materialises the proposed ticket files into `tickets/01_todo/`, and
then drives them through the standard ticket loop one at a time
(`underwrap-ticket-workflow` describes that loop for sessions that have
it available). This skill does not invoke the execution loop — the
user starts a fresh session for execution so the review context doesn't
bleed into the implementation.

When a ticket's findings are mostly category 7a (Simplification), the
ticket's acceptance section should explicitly call out a final
simplification pass on the touched files before closing: re-read each
file against the category 7a bullet list above and remove anything that
still fits. This is just the same lens, applied once more after the
mechanical refactor lands.
