---
name: underwrap-ticket-workflow
description: Ticket-driven workflow for implementation work in the underwrap repository. Use when asked to pick up, advance, or complete a ticket under tickets/.
compatibility: Intended for the underwrap repository layout with tickets/01_todo, tickets/02_inprogress, and tickets/03_done.
---

# Underwrap ticket workflow

Use this skill only for implementation work already tracked under `tickets/`.
Follow `AGENTS.md`, especially its release posture, threat model, security rules,
and validation matrix.

## Before moving a ticket

1. Run `git status --short`.
2. Identify pre-existing user changes and leave them untouched.
3. Read the ticket, its parent story/epic if present, and every relevant source
   or documentation file.
4. Check `tickets/01_todo/` and `tickets/02_inprogress/` for overlapping work.
5. Move only the selected ticket from `01_todo` to `02_inprogress`.
6. Send the user a short scope and validation plan.

A dirty worktree is not permission to stage, overwrite, or commit unrelated
changes. If the selected work overlaps an unexplained change, stop and ask.

## Per-ticket loop

For one task ticket at a time:

1. **Develop** the smallest coherent ticket-shaped change.
2. **Review** the diff against the ticket acceptance criteria and `AGENTS.md`.
3. **Validate** with focused checks, then all required boundary checks.
4. **Close** by moving the ticket to `tickets/03_done/` only after acceptance
   criteria pass.
5. **Commit** the implementation and that ticket move together. Stage explicit
   paths and inspect `git diff --cached` before committing.

Do not batch completed tickets into one commit unless the user asks. If the
scope expands into a new product, architecture, dependency, or security
choice, stop and ask rather than silently absorbing it.

## Validation examples

Choose checks from the matrix in `AGENTS.md`. Common commands are:

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

## New and renamed files

Nix evaluates a Git-filtered source snapshot, so untracked files are absent.
Before a Nix check that needs a new file:

1. confirm the file belongs to the ticket;
2. add only that file (or use `git add -N <path>` while reviewing the diff);
3. re-run `git status --short`;
4. never stage unrelated untracked files just to make the flake see them.

## Ticket numbering

When materializing an approved review plan, use a free four-digit `NNN0_` block
at or above `1000_`, checking all three board directories. The canonical naming
rules live in `AGENTS.md`.
