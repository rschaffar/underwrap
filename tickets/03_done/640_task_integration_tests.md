# Task: Integration test coverage for the new runtime API

Parent: `600_story_runtime_api_redesign.md`

## Scope

Comprehensive integration coverage of the three new endpoints in
`crates/integration-tests`. Cover positive paths, negative paths,
the env-immutability rule, concurrent invocations, and the trust
boundaries we want to keep pinned forever (daemon-bypass tests).

The existing `harness::TestDaemon::direct_service_client` helper
already exists from the previous round; use it to hit the new
endpoints directly with mTLS but without the daemon.

### Test inventory

In `crates/integration-tests/src/tests/runtime_api.rs` (new file):

**Happy path**:

- `register_creates_resources_and_pairs_when_auto_create_enabled`
- `register_refuses_with_structured_missing_lists_when_auto_create_disabled`
- `register_is_idempotent_on_repeat_call`
- `open_invocation_returns_blob_and_writable_for_unlocked_permission`
- `open_invocation_returns_blob_for_unlocked_until_unexpired_permission`
- `finalize_writes_blob_and_bumps_last_used_when_writable`
- `finalize_without_blob_only_bumps_last_used`

**Policy / authorization**:

- `open_invocation_denies_when_no_permission_exists`
- `open_invocation_denies_locked_permission`
- `open_invocation_denies_expired_unlocked_until`
- `open_invocation_denies_when_pair_is_not_registered`
- `finalize_denies_when_no_permission_exists`
- `finalize_denies_writeback_on_readonly_permission`
- `finalize_denies_writeback_when_permission_was_revoked_after_open`
- `finalize_denies_writeback_when_writable_was_flipped_off_after_open`

**Env immutability (writeback validation)**:

- `finalize_rejects_blob_whose_env_section_was_mutated`
  — open, mutate env in the returned blob, finalize → InvalidBlob
  with `reason: "env_section_immutable"`. Stored blob unchanged.
- `finalize_accepts_blob_with_modified_files_section_unchanged_env`
- `finalize_accepts_blob_with_modified_libsecret_section_unchanged_env`
- `finalize_rejects_blob_with_env_key_added`
- `finalize_rejects_blob_with_env_key_removed`
- `finalize_rejects_blob_with_env_value_changed`

**Blob shape**:

- `finalize_rejects_invalid_blob_bytes` (random base64)
- `finalize_rejects_noncanonical_blob_bytes` (valid blob but JSON
  key reordered)
- `finalize_rejects_unknown_schema_version`

**Trust-boundary regression (daemon-bypass)**:

- `direct_finalize_with_mutated_env_is_rejected_at_backend` —
  mTLS client constructed directly, bypassing the daemon entirely;
  attempts a finalize that mutates env → 400 InvalidBlob. Same
  spirit as the existing
  `backend_rejects_writes_on_readonly_permission_when_daemon_is_bypassed`.
- `direct_finalize_on_readonly_permission_is_rejected_at_backend`
- `direct_finalize_with_noncanonical_blob_is_rejected_at_backend`
- `runtime_client_cannot_address_underwrap_keys` — assert that
  there is **no** wire path for a runtime mTLS client to write to
  `__underwrap/policy/document` or any other internal key. The test
  attempts every conceivable shape (POST to /v1/wrappers with
  injected fields, POST to /v1/wrapper-invocations with a crafted
  resource_name containing `..` or `__underwrap`, etc.) and asserts
  each is rejected.
- `runtime_client_cannot_address_legacy_per_key_shape` — same
  spirit; assert `<resource>/<slot>/<key>` shapes can't sneak through
  via `resource_name`.

**Concurrency**:

- `two_concurrent_invocations_on_same_resource_serialize_last_writer_wins`
- `concurrent_open_calls_each_get_consistent_blobs`
- `last_used_at_advances_on_every_invocation`

**Enrollment / lazy self-heal regression** (pre-existing behaviour,
keep working):

- `daemon_recovers_when_backend_starts_late`
- `daemon_recovers_when_credential_pem_arrives_late`

**No-SSH-approval regression** (assert removal):

- `policy_document_with_per_call_approval_mode_is_rejected` —
  attempting to write a `PerCallApproval`-shaped policy doc via the
  admin API now fails (the variant doesn't exist).
- `grep_for_ssh_crypto_in_daemon_returns_nothing` (cargo-deny / cargo-machete in CI, not a Rust test — file under `nix/checks` or `.github` instead).

### Acceptance criteria

- [ ] All listed tests exist in
      `crates/integration-tests/src/tests/runtime_api.rs` (or split
      into logical files: `runtime_api_open.rs`, `runtime_api_finalize.rs`,
      `runtime_api_env_immutability.rs`,
      `runtime_api_trust_boundary.rs`,
      `runtime_api_concurrency.rs`).
- [ ] Every test that exercises a denial asserts the structured
      `reason` string, not just the HTTP status.
- [ ] `cargo test -p integration-tests` green.
- [ ] No test still relies on the legacy `/v1/read`, `/v1/write`,
      `/v1/list`, `/v1/read-prefixes`, `/v1/delete`, `/v1/status`,
      or `/v1/runtime/wrappers/register` endpoints.
- [ ] The pre-existing daemon-bypass tests
      (`backend_rejects_writes_on_readonly_permission_when_daemon_is_bypassed`
      etc.) are rewritten against the new endpoints.

### Out of scope

- VM test — `650_task_vm_test_update.md`.
- Unit tests inside individual crates — those land with their
  respective code tickets.
