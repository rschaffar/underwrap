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

## Posture: still greenfield, aiming for a clean first release

This project has **not shipped a first release yet**. There is no
external user base, no compatibility surface, and no migration burden.
The goal of every review pass is to make the first release look like the
code was written this way from the start.

Judgement calls follow from that:

- **No backwards-compatibility concerns.** It is fine — and often
  preferable — to rename types, change config schemas, restructure the
  NixOS module surface, and reshape IPC wire types when doing so
  makes the first release cleaner. Finished work does **not** create
  a constraint; only the code as it stands does.
- **Replace awkward early designs rather than paper over them.** If a
  module exists because of how something was done six months ago and
  there is now a clearly better shape, propose the replacement, not
  the polish.
- **No legacy/parallel modes.** Per the user's standing feedback, do
  not propose intermediate "live alongside the old thing" states.
  Cutovers happen in one ticket where reasonable.
- **No ticket references in the code.** The codebase must not carry
  pointers into the ticket tree — no `// see ticket NNNN`, no
  `// TODO(NNNN)` referencing a ticket number, no commit-hash
  references, no `// from epic_<name>`, no doc comments naming
  tickets, no module headers naming the epic that introduced them. Tickets and commit messages are the right place
  for that context; the code is not. Treat every such reference as a
  finding and route it through the project-wide sweep below.
- **No migration / old-version support.** There is nothing deployed
  yet, so there is nothing to migrate from. Flag and remove any code
  whose only purpose is to handle a previous shape of the world:
  - schema-version fields, `version: u32` discriminators, or
    `match version { 1 => ..., 2 => ... }` branches when only one
    shape was ever live
  - `serde(default)` / `Option<T>` fields that exist only to tolerate
    older serialized blobs
  - protocol-negotiation code, capability flags, or "fallback to old
    behaviour" arms that were added pre-first-release
  - database / store migration steps that could be folded into the
    initial schema
  - "kept for backwards compat with X" comments, transitional helper
    functions, dual-write paths, dual-read paths
  - retired-format constants, enum variants marked deprecated, types
    suffixed `V1`/`Legacy`/`Old`
  Code we ship at the first release is **version 1, full stop.**
  Anything that hedges against an earlier state is dead weight to
  remove now while it's still cheap. Real versioning will start
  *after* the first release, when there are actual deployed clients
  to migrate.
- **Security boundaries are not greenfield.** The "Hard rules" section
  below still applies — refactor freedom never extends to weakening a
  security mechanism described in `CLAUDE.md` "Security-critical
  rules" or in `docs/security.md`.

## Inputs

The user names a **component**. A component is one of:

- a single crate (e.g. `crates/underwrap-exec`)
- a module subtree inside a crate (e.g.
  `crates/underwrapd/src/wrapper_flow/`)
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
5. **Test layout & coverage** — `AGENTS.md` mandates the
   **sibling-file `tests.rs` submodule pattern**: production code never
   carries an inline `#[cfg(test)] mod tests { ... }` block at the
   bottom of the file. Tests live in a sibling file
   (`foo/tests.rs` for `foo.rs`), declared from the production file
   with a one-line `#[cfg(test)] mod tests;`. Cross-module helpers go
   in `src/test_support.rs`, single-file helpers stay in that file's
   `tests.rs`. Flag, with location, every occurrence of:
   - inline `mod tests { ... }` blocks. The rule is unconditional —
     every occurrence, regardless of length, is a violation.
   - `#[cfg(test)] pub(crate) use ...` re-exports at the crate root
     that exist purely so an inline test block can reach moved code
     (explicitly forbidden by `AGENTS.md`)
   - `tests.rs` files over ~600 lines that should be split
   - missing `test_support.rs` when the same fixture is hand-rolled in
     multiple `tests.rs` siblings
   - behavioural coverage gaps in the production code in scope
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
   replaceable, and **ticket references inside source/config/docs**
   (see the "Posture" section above). Any `// see ticket NNN`,
   `// TODO(NNNN)` with a ticket number, `// from epic_*`, or
   doc-comment / module-header pointer to a ticket is a finding.
10. **Doc & comment hygiene** — missing rustdoc on `pub` items,
    WHAT-comments that should be deleted (per the global comment rule),
    stale references to renamed things.

### 2a. Project-wide sweep for repo-conventions

Some categories above describe repo-wide conventions — when a finding
shows the convention is violated inside the component in scope, the
**same violation almost certainly exists in other components too**, and
fixing it once per review is wasteful. Before writing the report, run a
quick repo-wide grep for each of the following and fold every hit into
the findings list (not just the ones inside the component scope):

- **Inline `mod tests {}` blocks** — `rg -n '^#\[cfg\(test\)\][[:space:]]*\nmod tests \{' crates/`
  or equivalent. Every match becomes a finding flagged
  **`scope: project-wide`**. The rule has no exceptions.
- **`#[cfg(test)] pub(crate) use` re-exports at crate roots** —
  `rg -n '#\[cfg\(test\)\][[:space:]]*\n[[:space:]]*pub\(crate\) use' crates/`.
  All hits are findings; the rule has no exceptions.
- **`tests.rs` files over ~600 lines** — `find crates -name tests.rs -size +24k`
  as a rough proxy, then verify line count.
- **Ticket references in code, config, or docs** — `rg -n -i
  '(ticket[[:space:]]*[0-9]{3,4}|epic_[a-z_]+|story_[a-z_]+|TODO\([0-9]{3,4}\)|FIXME\([0-9]{3,4}\))'
  crates/ web/ nix/ docs/ examples/`. Every hit is a finding;
  see-also `git grep` for anything inside `.md` files under
  `docs/`. The rule has no exceptions: tickets and commits carry
  history, code does not.
- **Old-version / migration support** — none should exist yet (see
  the "Posture" section). Sweep with patterns such as:
  - `rg -n -i '(backward|backwards|backcompat|back-compat|legacy|deprecated|migration|migrate_from)' crates/ web/ nix/ docs/`
  - `rg -n '\b(V[0-9]+|Legacy|Old)\b' crates/` for types/enum
    variants suffixed with version markers
  - `rg -n 'serde\(default' crates/` and check whether each
    `#[serde(default)]` exists to tolerate an older blob or has a
    real present-tense reason
  - `rg -n 'schema_?version|protocol_?version|format_?version' crates/`
  - `rg -n 'TODO.*(remove|drop|delete).*(old|legacy|once|after.*release)' crates/`
  Each hit must be examined: is this guarding against a previous
  shape that was never deployed? If yes, finding; route as a
  pure-refactor removal (no behaviour change because the old
  behaviour was never reachable). If a hit turns out to guard a
  *current* shape, note it as a false positive in the report and
  move on — do not produce a finding.
- **WHAT-comments** that just narrate the next line of code, when seen
  in scope and clearly part of a wider habit — note this as a
  project-wide sweep candidate but do not enumerate every occurrence
  outside scope (judgement: too noisy to grep mechanically).

Findings produced by this sweep that fall outside the named component
get their own ticket in the plan, separate from the in-scope tickets,
with the title prefix `repo_sweep_` (e.g.
`1030_task_repo_sweep_relocate_inline_tests.md`). One sweep ticket per
convention, not one per offending file.

Repo-wide sweep findings must still cite `file:line` for every
occurrence and must use the same scope-discipline routing as in-scope
findings (pure refactor only; no behaviour change).

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
- K must-fix, L nice-to-have.
- Proposed ticket shape: <epic + tasks | flat tasks> (see "Ticket plan" below).
- Every finding becomes a ticket. There is no deferral bucket.

## Findings

### F-001 — <short title>

- **Category:** <one of the ten>
- **Severity:** must-fix | nice-to-have
- **Scope:** in-component | project-wide
- **Location(s):** path/to/file.rs:NN-NN (list all sites for
  project-wide findings)
- **Observation:** <what is there now, quoting the relevant lines>
- **Recommendation:** <concrete change>
- **Behavior change?** no | yes — <why it's still safe>
- **Related tickets:** <links to existing tickets, if any>

[... F-002, F-003, ...]

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
  movement). Lay out one `epic_<component>_refinement.md`, one
  `NNNN_story_<component>_refinement.md`, and the `NNNN_task_*.md`
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

## Scope discipline

Every finding becomes a ticket — no deferral, no "routed elsewhere"
bucket. The `Behavior change?` field is a *label*, not a routing
decision: it tells the executor which acceptance checks need to
catch the diff (wire-format tests, the VM test, etc.), not whether
to write the ticket.

When a refactor genuinely needs to land in two steps (e.g. add the
new shape, then delete the old one) split it into multiple sequential
tickets in the same plan rather than punting the second half to the
user. Sequencing inside the plan is fine; deferring is not.

Tickets emitted by this skill include, without distinction:

- pure refactors: moves, renames, file splits, dead-code removal,
  test relocation, doc cleanups
- API-hygiene tightening (`pub` → `pub(crate)`, removing leaky
  re-exports)
- correctness wins: better `anyhow` context, missing `zeroize`,
  missed return-value checks, log-level fixes, missing rustdoc on
  `pub` items
- bug fixes with observable behaviour change
- wire-format, NixOS module structure, or config-schema changes
  (greenfield posture permits them)
- security mechanism changes — emit the ticket *and* note in its
  acceptance check that `cargo test -p integration-tests` plus the
  VM test must stay green, and quote the relevant `CLAUDE.md` /
  `docs/security.md` invariant the change must not violate
- cross-component architectural rework — package it as its own epic
  in the same plan

## Hard rules

- **Never edit production code in this skill.** Reading and grepping
  only.
- **Never move tickets or write into `tickets/01_todo/`,
  `tickets/02_inprogress/`, or `tickets/03_done/`.** This skill only
  writes under `tickets/_review/`.
- **Never propose a ticket that weakens a security boundary** described
  in `CLAUDE.md` "Security-critical rules" or in `docs/security.md`.
  Tickets that *touch* one of those boundaries are still emitted, but
  the ticket body must quote the relevant invariant and the acceptance
  check must include `cargo test -p integration-tests` plus the VM
  test so a regression is caught before merge.
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
- If a finding involves a judgement call (e.g. "rename `XyzClient` to
  `XyzRuntimeClient` everywhere"), pick the option you would
  recommend, explain *why* in the ticket body, and emit the ticket.
  Do not mark findings "decision needed" and punt — that is a
  deferral, which this skill does not do.
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
