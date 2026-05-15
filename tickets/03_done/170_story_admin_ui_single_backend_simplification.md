# Story: Simplify the admin UI to a single-backend model

Parent: none
Epic: follow-up work after `epic_nextjs_admin_ui_replatform.md`

## Background

The current Next.js admin UI still models a top-level **target** selector and a
`backends.<name>` map in runtime config. That abstraction is wrong for the
operator UI.

The user clarified the intended architecture:

- frontend clients / wrappers may need to read from multiple remote backends
- that multi-backend concern belongs on the **client/runtime side**
- each backend has its **own admin UI/frontend**
- therefore the operator UI should manage exactly **one backend**

So the admin UI should not expose or require a multi-target backend chooser.

## Desired design

- one admin UI instance fronts one backend admin API
- UI runtime config uses a singular `backend` object, not `backends.*`
- no target switcher in the UI
- no `/t/[target]/...` route layer
- store remains the top-level navigation scope inside one backend
- deployment config becomes simpler and less error-prone

## Handoff notes / current repo state

Current underwrap repo still has the old multi-target admin UI model:

- route layout includes `/t/[target]/...`
- shell renders a target switcher
- server config loads `backends: { <name>: ... }`
- backend-client helpers still support multiple configured backend clients
- `~/nix/my_nix_os/hosts/shame01/configuration.nix` currently uses a single
  backend named `robert`, which is confusing because it is really just the one
  backend/admin endpoint for the host

The user explicitly wants this removed from the admin side entirely.

## Acceptance criteria

- [ ] Admin UI runtime config uses a single `backend`, not a backend map.
- [ ] The target selector is removed from the UI shell.
- [ ] App routes no longer require `/t/[target]/...`.
- [ ] Store-scoped pages still work after the flattening.
- [ ] `~/nix/my_nix_os` deployment config is updated to the simplified model.
- [ ] Tests and docs reflect the single-backend admin UI design.
