---
name: underwrap-ticket-workflow
description: Ticket-driven workflow for implementation work in the underwrap repository. Use when asked to pick up, advance, or complete a ticket under tickets/.
compatibility: Intended for the underwrap repository layout with tickets/01_todo, tickets/02_inprogress, and tickets/03_done.
---

# Underwrap Ticket Workflow

Use this skill for implementation work tracked as tickets in this repository.

## Default loop

For each task ticket:

1. Develop
2. Test
3. Close the ticket
4. Commit

Do not batch multiple completed tickets into one commit by default.

## Required per-ticket routine

When starting the next ticket:

1. Move the ticket from `tickets/01_todo/` to `tickets/02_inprogress/`.
2. Read the ticket and relevant code/docs.
3. Send a short mini plan explaining what you are about to do.
4. Implement the ticket.
5. Run the relevant checks/tests.
6. Move the ticket to `tickets/03_done/`.
7. Commit the code and ticket move together.

## Working style expectations

- Prefer small, clean, ticket-shaped commits.
- Keep the user informed briefly at ticket boundaries.
- If a task grows into a clearly larger cross-cutting slice, pause and say so.
- If an important product or security question appears, stop and ask before continuing.
- Follow best practices and refresh on docs/references when needed.

## Ticket numbering

When creating new ticket files (e.g. materialising a plan from
`tickets/_review/` produced by `underwrap-component-refine`), use a
zero-padded **4-digit** prefix at or above `1000_`. Pick a free `NNN0_`
block that doesn't collide with anything in `tickets/01_todo/`,
`tickets/02_inprogress/`, or `tickets/03_done/`. The canonical statement
of this rule lives in `AGENTS.md` under "Ticket system".

## Testing expectation

Before closing a ticket, run the checks relevant to the changed area.

Examples used in this project:

- `cargo test -p <crate>`
- `cd apps/admin-ui && npm test && npm run build`
- `nix build .#underwrap-admin-ui-nextjs --no-link`

## Important repo/Nix gotcha

When adding new frontend files, make sure they are git-tracked before relying on `nix build`. The flake source snapshot will not include untracked files, which can make Nix builds fail even when local frontend builds work.
