# Task: Apply revoke, remove, and remap on the next runtime request

Parent: `350_story_single_credential_reenrollment_and_immediate_effect.md`

## Scope

Ensure runtime access reflects current backend state on the next request.

## Acceptance criteria

- [ ] Removed or missing credentials deny on the next runtime request.
- [ ] Disabled instances deny on the next runtime request.
- [ ] Store remap applies on the next runtime request.
- [ ] Existing long-lived connections do not preserve stale authorization across later requests.
