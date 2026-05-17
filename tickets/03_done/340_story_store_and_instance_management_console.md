# Story: Stores and client-instance management console

Parent: `epic_dynamic_runtime_clients_enrollment_and_stores.md`

## Background

Once stores and client instances are backend-managed, the admin SPA needs a clear control surface for them. This is core product usability, not polish.

## Desired design

- Add a top-level Stores section that can:
  - list stores
  - create stores
  - edit store metadata
  - delete stores when allowed
- Add a top-level Client Instances section that can:
  - list instances
  - show display name first
  - show hostname hint and attested instance name as secondary metadata
  - show mapped store, status, and last seen
  - remap an instance to another store with confirmation
  - revoke/disable an instance
- Enrollment deep links should redirect through login and return to the intended page.
- Revoked/disabled instances remain visible in the normal list with filtering.

## Tasks

1. `341_task_add_store_management_view_models_and_endpoints.md`
2. `342_task_add_client_instance_management_view_models_and_endpoints.md`
3. `343_task_build_web_stores_management_section.md`
4. `344_task_build_web_client_instances_management_section.md`
5. `345_task_management_console_login_return_and_filtering_coverage.md`

## Acceptance criteria

- [ ] Stores are manageable from a dedicated top-level section.
- [ ] Client instances are manageable from a dedicated top-level section.
- [ ] Enrollment deep links survive login and return correctly.
- [ ] Instance remap is explicit and immediate.
- [ ] Display name is primary; hostname and attested instance name are still visible.
- [ ] Revoked/disabled instances remain visible with filter support.
