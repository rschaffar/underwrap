# Story: Build the resource explorer, secret reveal UX, and mutation workflow

Epic: `tickets/01_todo/epic_nextjs_admin_ui_replatform.md`

## Background

The new UI must support revealing secret values, not just listing and
writing entries. This is the most security-sensitive UX surface in the
rewrite and should be designed deliberately rather than copied from the
current form set.

## Scope

- Build the target/store/resource browsing flow.
- Add resource search and selection UX.
- Build resource detail views for:
  - env entries
  - files entries
  - libsecret entries
- Add masked-by-default secret reveal flows.
- Ensure reveal actions never place secret values in URLs.
- Handle non-UTF-8 material safely, e.g. text preview when valid UTF-8,
  base64/download flow otherwise.
- Integrate mutation-gate workflows:
  - show current mutation state
  - enable short-lived mutation window
  - perform write/update
  - revoke mutation window
- Support delete operations.

## Acceptance criteria

- [ ] Operators can navigate to a resource and inspect its slots.
- [ ] Operators can reveal a secret value through an explicit action.
- [ ] Revealed secrets are masked by default before the reveal action.
- [ ] Reveal flows do not leak values into URLs.
- [ ] Non-UTF-8 values are handled safely instead of being corrupted or
      assumed to be text.
- [ ] Operators can perform the enable-mutation -> write -> revoke flow
      from the new UI.
- [ ] Automated tests cover reveal, write, and mutation-gate rejection
      behavior.

## Child tasks

- `090_task_admin_ui_resource_explorer_and_navigation.md`
- `100_task_admin_ui_secret_reveal_and_mutation_flow.md`
