# Story: Build the wrapper mode console and modern access-matrix UX

Epic: `tickets/01_todo/epic_nextjs_admin_ui_replatform.md`

## Background

Wrapper modes and the access matrix are core operator workflows, but the
current UI exposes them as dense form-heavy HTML. This rewrite should
make those workflows clearer, faster, and more legible.

## Scope

- Build a filterable wrappers table.
- Show wrapper mode with strong visual state cues.
- Support editing wrapper mode for:
  - locked
  - unlocked
  - unlocked until
  - per-call approval
- Replace raw epoch editing with a real date/time UX.
- Validate and present approval-key input clearly.
- Build a better access-matrix view with filtering for resource, host,
  user, and wrapper.
- Support cell toggling for allow rules and make state transitions
  understandable to operators.
- Include mutation-related visibility where it belongs in the operator
  workflow.

## Acceptance criteria

- [ ] Operators can browse wrappers in a modern, filterable table.
- [ ] Operators can change wrapper mode without editing raw epochs.
- [ ] Operators can manage per-call approval keys from the new UI.
- [ ] Operators can inspect and toggle access-matrix cells with better
      clarity than the current UI.
- [ ] The UI makes it obvious which resource and wrapper scope an edit
      applies to.
- [ ] Automated tests cover wrapper mode edits and access-matrix
      toggles.

## Child tasks

- `110_task_admin_ui_wrapper_modes_and_access_matrix_console.md`
