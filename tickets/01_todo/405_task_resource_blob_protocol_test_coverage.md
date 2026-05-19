# Task: Resource blob and protocol test coverage

Parent: `400_story_resource_blob_storage_and_protocol.md`

## Scope

Land integration tests that exercise the new storage layout and the
new wrapper-side IPC verbs against a real daemon + backend.

### Cargo integration tests (in `crates/integration-tests/`)

New file `tests/resource_blob.rs`:

- `checkout_returns_canonical_blob_and_writable_flag` — seed a
  resource via the admin PUT, run a Checkout via a test fixture
  wrapper handle, assert the returned bytes equal
  `to_canonical_bytes` of the seeded value and `writable` matches
  the grant.
- `checkin_writes_back_atomically` — Checkin a modified blob,
  Checkout again, assert equality. While the Checkin is in flight,
  a second Checkout (sequenced with a barrier) must see either
  the old or the new bytes — never partial.
- `checkin_rejected_on_readonly_grant` — Checkin against a grant
  with `writable=false` fails with `Error::Unauthorized` and the
  stored blob is unchanged.
- `checkin_rejected_on_noncanonical_bytes` — server rejects a
  body whose bytes are not equal to `to_canonical_bytes` of the
  decoded form.
- `usage_signal_succeeds_without_payload` — verifies the bump
  path is reachable without a blob.
- `unknown_schema_version_rejected` — both wrapper-side and
  admin-side reject.

### Backend handler unit tests (in `backend-backendd/`)

Round-trip + decode/encode tests for the three admin endpoints.

### Storage backend unit tests (in `shared-store/`)

- `plain_backend_one_file_per_resource`.
- `encrypted_backend_one_nonce_per_resource`.
- `list_resources_prefix_returns_expected_set`.
- `read_prefixes_resources_returns_full_blobs`.

## Acceptance criteria

- [ ] All tests listed above exist and pass.
- [ ] `cargo test --workspace` green; clippy clean.
- [ ] No flaky concurrency tests — the interleaved-write test uses a
      deterministic barrier, not a sleep.

## Out of scope

- Tests for wrapper-end-of-run decision and SPA — stories 410 / 420.
- VM test — task 432.
