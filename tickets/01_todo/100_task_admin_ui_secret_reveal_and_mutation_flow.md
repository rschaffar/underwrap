# Task: Implement secret reveal, binary handling, and mutation edit flow

Parent: `090_story_resource_explorer_secret_reveal_and_mutation_flow.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Add explicit reveal actions for secret values.
- Keep values masked by default and support simple reveal/copy UX.
- Handle UTF-8 vs. binary material safely.
- Support real file upload/download for file-backed secrets with a 1 MiB cap.
- Ensure reveal/download actions are explicitly audited by the backend.
- Implement the operator flow for enable-mutation -> write -> revoke.
- Support delete operations in the new resource UI.

## Acceptance criteria

- [ ] Operators can explicitly reveal secret values.
- [ ] Reveals are masked by default, support simple reveal/copy, and never leak into URLs.
- [ ] Binary material has a safe base64/download path.
- [ ] File-backed secrets support real upload/download with a 1 MiB cap.
- [ ] Reveal/download actions are explicitly auditable.
- [ ] Mutation enable/write/revoke is supported end-to-end.
- [ ] Tests cover reveal, upload/download, write, delete, and mutation-gate rejection.
