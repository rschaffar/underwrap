# Task: Replace catalog publish with runtime auto-create endpoint

Parent: `380_story_backend_resource_and_pair_entities.md`

## Scope

Replace `POST /v1/catalog/put` with a new runtime endpoint that
idempotently creates any missing Resource and Wrapper/Resource Pair
records, gated by the per-instance auto-create flag from
`383_task_per_instance_auto_create_optin_flag.md`.

Proposed shape (final naming during implementation):

```
POST /v1/runtime/wrappers/register
ServiceWrapperRegisterRequest {
  pairs: Vec<{ wrapper: String, resource: String }>,
}
ServiceWrapperRegisterResponse {
  created_resources: Vec<String>,
  created_pairs: Vec<{ wrapper, resource }>,
  rejected: Option<RejectReason>,  // e.g. AutoCreateDisabled
}
```

Behavior:

- Authn/authz via the existing mTLS principal resolution; the calling
  client instance becomes `created_by` on any new record.
- If the instance's auto-create flag is off and any of the named
  entities is missing → respond with `AutoCreateDisabled` plus the list
  of missing entries (operator-friendly diagnostics). No partial
  creation in that case.
- If the flag is on → create only the missing entities, leave existing
  records alone (provenance immutable). Existing pairs/resources for
  entries the wrapper has since *removed* from its local config are
  left in place — this endpoint is upsert-only.
- Daemon caller in `crates/frontend-underwrapd/src/server.rs` updated
  to build the request from the wrappers configured for this instance
  and to log (not crash) on `AutoCreateDisabled`.

## Acceptance criteria

- [ ] New endpoint wired into `server.rs` routing.
- [ ] Daemon's `register_catalog()` replaced with a call to the new
      endpoint; function renamed accordingly.
- [ ] `AutoCreateDisabled` response shape exported to TS via ts-rs.
- [ ] Unit / integration tests cover the four cases: flag-off+all-exist
      (no-op success), flag-off+something-missing (refusal), flag-on+
      all-exist (no-op), flag-on+something-missing (creates only
      missing).
- [ ] No code path still references `/v1/catalog/put` or
      `register_catalog`.

## Out of scope

- The opt-in flag itself (separate task).
- Catalog blob removal (separate task — happens after this lands).
