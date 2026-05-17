# Story: Single-credential re-enrollment and immediate effect

Parent: `epic_dynamic_runtime_clients_enrollment_and_stores.md`

## Background

We do not want a full credential-rotation subsystem in the first release. Instead, each logical client instance should have exactly one current credential. Re-enrollment simply replaces it.

## Desired design

- Each logical client instance has exactly one current credential.
- There is no credential-history feature and no overlapping active credentials.
- Re-enrollment for an existing instance replaces the current credential for that same logical instance.
- If a credential is removed server-side, it is gone.
- Runtime access must reflect current backend state on the next request:
  - missing/removed credential → deny
  - disabled instance → deny
  - changed store mapping → new store applies
- Existing long-lived connections must not preserve stale authorization across later runtime requests.

## Acceptance criteria

- [ ] The runtime model supports exactly one current credential per logical instance.
- [ ] Re-enrollment replaces the credential for the same logical instance.
- [ ] Old credentials are not kept as a product feature/history surface.
- [ ] Revocation/removal takes effect on the next runtime request without requiring restarts.
- [ ] Store remap takes effect on the next runtime request without requiring restarts.
- [ ] Expired or removed credentials require re-enrollment rather than a separate rotation workflow.
