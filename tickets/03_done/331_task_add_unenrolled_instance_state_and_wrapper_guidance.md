# Task: Add unenrolled instance state and wrapper guidance

Parent: `330_story_invite_first_instance_enrollment.md`

## Scope

Introduce a first-class unenrolled client-instance state and make wrapper execution guide the operator into enrollment.

## Acceptance criteria

- [ ] A configured instance can exist without a runtime credential.
- [ ] Wrapper execution on an unenrolled instance prints a short explanation and a non-sensitive enrollment URL.
- [ ] The unenrolled path fails cleanly without pretending runtime access exists.
