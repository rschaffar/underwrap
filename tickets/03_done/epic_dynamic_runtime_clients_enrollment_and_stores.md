# Epic: Replace static backend clients and declarative stores with dynamic runtime clients, enrollment, and mapping

## Background

The current backend runtime model is still shaped like an early internal system:

- backend service clients are configured statically in YAML
- client identity is tied to certificate subject CN mapping
- backend stores can be declared statically on the server side
- certificates are provisioned manually

That is too awkward for first-release adoption. We want to keep the strong cryptographic runtime shape, but make the operator workflow substantially easier.

## Product decisions already made

- Keep the current broad architecture:
  - `underwrapd` on client machines
  - `underwrap-backendd` as the central backend
  - backend terminates HTTPS itself
  - one shared HTTPS listener for admin UI/enrollment and runtime API
- Keep certificate-based runtime client authentication.
- Do **not** keep static `service.clients` principal→store mapping.
- Do **not** keep declarative backend-managed stores.
- Replace manual provisioning with an invite-first enrollment flow.
- Model runtime identity per `services.underwrap.instances.<name>`.
- Each logical client instance has exactly one current credential at a time.
- Re-enrollment replaces the current credential; there is no separate rotation feature.
- Runtime access changes must take effect on the next request.
- Backward compatibility and migration helpers are not a constraint in this greenfield phase.

## Goals

1. Backend-managed stores with stable internal IDs and editable names.
2. Backend-managed runtime client-instance records and current credential records.
3. Dynamic instance→store mapping, editable from the UI.
4. Invite-first enrollment for unenrolled instances.
5. Shared-listener runtime auth that evaluates current backend state on each runtime request.
6. Re-enrollment and immediate revoke/remap behavior without credential-history complexity.

## Stories

Execution order:

1. `320_story_backend_managed_stores_and_runtime_instances.md`
2. `330_story_invite_first_instance_enrollment.md`
3. `340_story_store_and_instance_management_console.md`
4. `350_story_single_credential_reenrollment_and_immediate_effect.md`

## Acceptance criteria

- [ ] Static backend client mapping is removed from the runtime model.
- [ ] Backend stores are created and managed through backend state, not server-side declarative config.
- [ ] Runtime client identity is per client instance, not a static config principal.
- [ ] An unenrolled instance can guide an operator into the enrollment flow.
- [ ] An operator can choose or create a store during enrollment and later remap the instance.
- [ ] Each logical instance has exactly one current credential.
- [ ] Re-enrollment replaces the credential for the same logical instance.
- [ ] Revocation and store remap take effect on the next runtime request.
- [ ] Docs and tests reflect the new first-release model.
