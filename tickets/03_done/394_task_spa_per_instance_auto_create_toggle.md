# Task: SPA — per-instance auto-create toggle

Parent: `390_story_spa_wrappers_page_and_secrets_resource_management.md`

## Scope

Surface the `auto_create_wrapper_assets` flag added in
`383_task_per_instance_auto_create_optin_flag.md` on the Client
Instances page (`web/src/pages/client-instances.tsx`).

UI shape:

- The instance detail / edit dialog gains a checkbox **Auto-create
  wrapper assets on register**, with a short helper line: "When on,
  this instance's wrappers may add new resources and wrapper/resource
  pairs on startup. When off, the backend refuses to create them and
  logs which entries it expected." Default off, matching the backend
  default.
- The instance list row shows a small badge ("auto" / "manual") so an
  admin can see the per-instance state at a glance.

Changes go through the existing `postClientInstanceUpdate` call —
no new endpoint needed.

## Acceptance criteria

- [ ] Toggle visible and editable in the instance edit dialog.
- [ ] Badge visible in the list row.
- [ ] Vitest unit test asserts the field is sent through to the update
      endpoint.

## Out of scope

- Backend persistence (covered by 383).
- E2E (covered by 395).
