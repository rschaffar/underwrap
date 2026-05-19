# Task: Admin API for resource blob get and replace

Parent: `400_story_resource_blob_storage_and_protocol.md`

## Scope

Update the backend's HTTPS admin surface to read and write resource
blobs as whole documents instead of per-key get/set.

### Endpoints

Replace the existing per-key admin reveal / mutation endpoints for
the env / files / libsecret slots with:

- `GET  /v1/admin/resources/<resource_id>/blob`
  - Returns `{ blob: Vec<u8>, etag: u64 }` where `etag` is a
    server-side observation counter (monotonic per resource) used
    only for optimistic display logic in the SPA, not for
    concurrency control on Checkin.
- `PUT  /v1/admin/resources/<resource_id>/blob`
  - Body: `{ blob: Vec<u8> }`. Server decodes to `ResourceBlob`
    (rejects unknown schema_version), re-canonicalizes, refuses
    if input is not already canonical (operators are expected to
    send canonical bytes — the SPA reuses the same code path as
    the wrapper). Writes atomically.
- `GET  /v1/admin/resources/<resource_id>/decoded`
  - Convenience for the SPA's read-only views: decoded
    `ResourceBlob` as JSON. Operators with reveal permission only.

Existing CRUD on resource entity records (`ResourceRecord`,
`WrapperResourcePairRecord`) stays — this task touches *blob*
content, not entity provenance.

### Authz

- Reading the blob requires the operator's "reveal secrets" role on
  the store (same gate as today's per-key reveal endpoint).
- Writing the blob requires the existing "mutate secrets" role and
  is itself subject to the store-wide `writes_enabled` toggle.

### SPA call site

The SPA's Secrets page and resource editor now load the whole
decoded blob, present an editor over its sections (env table,
files tree, libsecret keyring), and save by canonicalizing the
edited blob client-side (ts-rs surfaces the type; canonicalization
runs through a thin wasm wrapper or, simpler, via a round-trip
through the backend's `PUT` validation — pick whichever is
simpler in 404 and document the choice).

## Acceptance criteria

- [ ] Three endpoints exist, with the auth gates wired to the
      existing role middleware.
- [ ] Endpoints round-trip a non-trivial blob; canonicalization
      enforcement rejects a hand-tweaked non-canonical body with
      a typed error.
- [ ] Old per-key reveal/mutation endpoints are deleted.
- [ ] ts-rs types regenerate cleanly; SPA build does not regress
      (the SPA wiring lands in story 420 / a later SPA story but
      the type surface compiles here).
- [ ] Unit + integration tests cover read, replace, schema-version
      reject, non-canonical reject, authz reject.

## Out of scope

- SPA editor UI for the new endpoints — separate ticket, not in
  this epic. This task only needs the type surface and integration
  tests; the existing SPA Secrets page can continue to render with
  minimal changes via the decoded endpoint.
- Per-pair `last_used` exposure — task 422.
