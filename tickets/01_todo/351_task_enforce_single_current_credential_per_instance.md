# Task: Enforce a single current credential per instance

Parent: `350_story_single_credential_reenrollment_and_immediate_effect.md`

## Scope

Make the runtime-client model support exactly one current credential per logical instance.

## Acceptance criteria

- [ ] A logical instance can have exactly one current credential.
- [ ] The active runtime path does not depend on credential-history or overlapping active credentials.
