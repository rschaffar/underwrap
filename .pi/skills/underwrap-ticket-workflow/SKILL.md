---
name: underwrap-ticket-workflow
description: Materialize explicitly approved underwrap review findings into epics/stories/tasks, or execute work already tracked under tickets/. Use only after a review report has been presented and separately approved for ticketing, or when asked to pick up, advance, or complete an existing ticket.
compatibility: Intended for the underwrap repository layout with tickets/01_todo, tickets/02_inprogress, tickets/03_done, and tickets/_review.
---

# Underwrap ticket workflow

This skill has two explicit entry modes:

1. **Report materialization** — derive and create ticket files from selected
   findings in an already-presented, explicitly approved consolidated review.
2. **Ticket execution** — implement work already tracked by one task ticket.

Never slide from review into materialization, or from materialization into
implementation, in one implicit flow. A report's existence is not approval.
Follow `AGENTS.md`, especially its release posture, threat model, security rules,
ticket naming, and validation matrix.

## Select one mode

Use report materialization only when the user explicitly asks to derive or
create tickets from a presented report. The request must identify the report and
approve either specific themes/findings or the whole report. Approval of the
whole report means its accepted critical/high/medium findings; it does not
include deferred low findings, unresolved/rejected items, or Opportunities.
Ask one focused question at a time when scope or behavior approval is ambiguous.

Use ticket execution only when the user asks to implement an existing ticket.
For a code review, use `underwrap-codebase-review` instead.

# Report materialization

## Approval gate

Establish all of these before deriving tickets:

- exact consolidated report under `tickets/_review/`;
- its controller presentation has already happened;
- a later explicit request authorizes materialization;
- approved remediation themes/findings are unambiguous;
- every intentional behavior-change Opportunity is separately approved.

A generic “continue” immediately after review is insufficient. Do not infer
approval from severity or reviewer recommendations. Keep low findings deferred
until a fresh follow-up review promotes them or the user explicitly selects
them. Never ticket rejected/unresolved items as accepted work.

## Materialization preflight

1. Run `git status --short` and `git diff --cached --name-only`. Record and
   preserve all pre-existing changes and staged paths.
2. Read `AGENTS.md`, the complete report, and current source/test/doc evidence
   needed to confirm selected findings still apply.
3. Compare the report baseline with the current tree. Revalidate selected
   findings touched by intervening changes; request a fresh review when the
   report is no longer a trustworthy basis.
4. Scan todo, in-progress, and done ticket directories for overlap and free
   numbering. Link active overlap instead of duplicating it. Verify a recurrence
   of completed work before creating new work.
5. Resolve dependencies and the smallest coherent implementation units before
   assigning filenames.

Materialization is read-only outside `tickets/01_todo/`. Do not edit the review,
production files, published docs, or existing tickets unless separately asked.
Do not modify or unstage pre-existing index entries.

## Derive coherent work

Preserve the synthesis rather than raw reviewer partitioning:

- combine findings that require one coherent correction, including findings
  from different perspectives;
- separate unrelated work even when it touches one file;
- preserve remediation-theme dependencies and order;
- do not create one task mechanically per finding;
- do not create miscellaneous-cleanup or remaining-polish grab bags;
- map every selected finding to an existing active ticket or at least one new
  task;
- record report path, finding ids, and theme ids in each new story/task;
- state whether behavior is preserved; approved Opportunities state the exact
  behavior change and non-goals.

Choose hierarchy by cohesion:

- **Task only** — one independently deliverable correction.
- **Story + tasks** — several implementation units serving one observable
  technical or operator outcome.
- **Epic + stories + tasks** — several story-sized outcomes spanning components,
  security boundaries, or ordered phases.

An epic is not a bucket for the entire review. Use only the hierarchy needed to
communicate outcomes, dependencies, and completion.

## Numbering and filenames

Check all three board directories. Story/task filenames use a free four-digit
`NNN0_` block at or above `1000_`; lower numbers indicate intended order.

- `epic_<name>.md`
- `NNNN_story_<name>.md`
- `NNNN_task_<name>.md`

Reserve one coherent block for the materialization. Do not renumber existing
work merely to make the block contiguous.

## Ticket content

Every implementation task contains at least:

```markdown
# <Task title>

Parent: <story path or none>
Source review: <report path>
Source findings: <F-ids>
Source themes: <C-ids or none>

## Outcome
<observable result and why these findings belong together>

## Scope
<smallest coherent implementation boundary>

## Non-goals
<behavior or adjacent work deliberately excluded>

## Acceptance criteria
- <observable, testable criterion>

## Security boundary
None, or:
- Protected asset: ...
- Attacker: ...
- Boundary direction: ...
- Invariant: <quote or precise link to AGENTS.md/docs/security.md>

## Validation
- `<exact focused command>`
- `<integration/browser/VM command required by the boundary>`

## Dependencies
- <ticket paths or none>
```

Stories define their shared outcome, cross-task acceptance, ordering, and child
links. Epics define the cross-story objective, completion condition, non-goals,
ordering, and story links. Link the source report rather than duplicating it.

Security-sensitive tasks quote the relevant invariant, identify
asset/attacker/boundary direction, and include integration plus VM validation
when that boundary is exercised there. A finding does not pre-approve a new
dependency, a change to the `underwrap-exec → underwrap-ns` binary chain, or a
weaker security boundary.

## Materialization completion

1. Create only approved epic/story/task files under `tickets/01_todo/`.
2. Verify filenames, links, selected-finding coverage, dependencies, and order.
3. Directly check every new untracked ticket for trailing whitespace, final
   newline, readable Markdown structure, and unresolved placeholders. Ordinary
   `git diff --check` does not include untracked files; do not rely on it alone.
4. Inspect `git status --short` and present:
   - every created path/title;
   - finding/theme-to-ticket mapping;
   - active-ticket overlap;
   - approved findings not materialized and why;
   - deferred low findings and Opportunities still awaiting follow-up or
     behavior approval.
5. Stop without moving a task to in-progress or implementing it.

Do not commit ticket files unless the user explicitly asks. If a materialization
commit is requested, require that the index was empty at preflight; otherwise
stop and ask the user to resolve their staged work. Stage only the exact created
ticket paths, verify `git diff --cached --name-only` exactly equals that
allowlist, inspect `git diff --cached`, then commit. Never include or unstage an
unrelated path.

# Ticket execution

## Before moving a task

1. Run `git status --short` and `git diff --cached --name-only`.
2. Identify and preserve pre-existing changes. If any staged path is outside the
   selected task's explained scope, stop before moving the ticket and ask the
   user to resolve it; never unstage their work.
3. Read the task, parent story/epic, linked source-review findings, and every
   relevant source/test/doc file.
4. Check todo and in-progress tickets for overlap.
5. Confirm the task remains coherent and acceptance is testable. Stop for an
   unapproved product, architecture, dependency, binary-chain, or security
   decision.
6. Move only the selected task from todo to in-progress.
7. Send a concise scope and validation plan.

A dirty tree is not permission to overwrite, stage, or commit unrelated work. If
an unexplained change overlaps the task, stop and ask.

## Per-task loop

1. **Develop** the smallest coherent ticket-shaped change.
2. **Review** the actual diff against acceptance, source findings, and
   `AGENTS.md`.
3. **Validate** with focused checks and every required boundary check.
4. **Close** by moving the task to done only after acceptance passes.
5. **Commit** implementation and task move together:
   - enumerate the exact intended path allowlist, including old/new task paths;
   - stage only that allowlist;
   - require `git diff --cached --name-only` to equal it exactly;
   - inspect `git diff --cached` for content and secrets;
   - commit only after those checks pass.

If an unrelated staged path appears, stop without committing or unstaging it.
Do not batch tasks unless asked. Do not absorb nearby review findings into the
active task; escalate or track them separately.

## Validation examples

Choose commands from `AGENTS.md`:

```bash
cargo test -p <crate>
cargo test -p integration-tests
cargo fmt -- --check
cargo clippy --all-targets --workspace -- -D warnings
cd web && npm test && npm run build
cd web && npm run test:e2e
nix build .#checks.x86_64-linux.vm-test
```

Run browser tests inside `nix develop`. NixOS module, wrapper identity,
namespace, capability, mount, or peer-auth changes require the VM test.

## New and renamed implementation files

Nix evaluates a Git-filtered source snapshot. Before a Nix check needing a new
file:

1. confirm it belongs to the selected task;
2. add only that file, or use `git add -N <path>` while reviewing;
3. re-run status and the exact staged-path check;
4. never stage unrelated untracked files merely to expose them to the flake.
