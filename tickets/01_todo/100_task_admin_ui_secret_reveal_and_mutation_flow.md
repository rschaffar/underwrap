# Task: Implement secret reveal, binary handling, and mutation edit flow

Parent: `090_story_resource_explorer_secret_reveal_and_mutation_flow.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Add explicit reveal actions for secret values.
- Keep values masked by default.
- Handle UTF-8 vs. binary material safely.
- Implement the operator flow for enable-mutation -> write -> revoke.
- Support delete operations in the new resource UI.

## Acceptance criteria

- [ ] Operators can explicitly reveal secret values.
- [ ] Reveals are masked by default and never leak into URLs.
- [ ] Binary material has a safe base64/download path.
- [ ] Mutation enable/write/revoke is supported end-to-end.
- [ ] Tests cover reveal, write, delete, and mutation-gate rejection.
