# Preserve untouched resource slots during flat secret mutations

Parent: none
Source review: tickets/_review/codebase_2026-07-18.md
Source findings: F-001
Source themes: C-001

## Outcome

Writing, deleting, or renaming one env/files entry through the admin API changes
only that selected entry and preserves every untouched env, files, and libsecret
value in the resource blob. This removes destructive cross-slot data loss while
preserving the existing successful flat-secret behavior.

## Scope

- Correct flat env/files writes and per-entry deletes in
  `storage/secret_entries` so their load-modify-save cycle retains the complete
  loaded typed `ResourceBlob`.
- Correct admin flat-entry rename, including rename-with-material and same-key
  material replacement, so it retains every section and every entry outside the
  selected key.
- Keep the backend-store flat projection coherent: it may continue to expose
  only env/files entries, but no flat mutation may replace a complete loaded
  blob with only that lossy projection.
- Add regression coverage that begins with a persisted resource containing
  populated env, files, and libsecret state and reloads the saved blob after
  every supported mutation.
- Preserve behavior. Existing valid API shapes, responses, selected-key
  semantics, canonical one-blob storage, and last-writer-wins concurrency remain
  unchanged.

## Non-goals

- Do not change whole-resource deletion blockers, ResourceRecord deletion, or
  empty-blob cleanup; those belong to F-011.
- Do not add resource/client/pair existence checks or change missing-entity
  behavior; those belong to F-014.
- Do not add revisions, locking, transactions, compare-and-swap, merge-on-
  conflict, or stronger concurrency claims; those require separate approval of
  Opportunity O-004.
- Do not change libsecret structured endpoint behavior, runtime writeback
  policy, the resource schema/version, the frontend, dependencies, or the
  `underwrap-exec → underwrap-ns` chain.

## Acceptance criteria

- A flat env/files write creates or replaces only its addressed key. After
  storage reload, every unrelated env/files entry and the complete pre-existing
  libsecret section are unchanged.
- Deleting an existing flat env/files entry removes only that key, preserves all
  other sections, and retains the existing `existed = true` result. Deleting a
  missing key returns `existed = false` without changing the resource.
- Renaming within env or files preserves the sibling flat map and complete
  libsecret section while retaining source removal, destination insertion,
  optional material replacement, same-key replacement, missing-source, and
  target-conflict behavior.
- Regression tests start from nontrivial mixed state: multiple env/files entries
  plus libsecret aliases, collections, items, attributes, timestamps/metadata,
  and secret bytes. Assertions compare the reloaded typed blob with an explicit
  expected blob rather than checking only flat keys or collection counts.
- A real admin-route regression exercises PUT, DELETE, and rename against mixed
  state and confirms structured libsecret data remains intact after each
  successful mutation.
- Resource blobs remain canonical, schema-versioned, and persisted as one
  resource key through `StorageBackend`; no secret material is logged or added
  to another persistence path.
- Existing backend-store, backend, integration, formatting, and lint checks pass.

## Security boundary

- Protected asset: integrity and availability of persisted env, files, and
  especially libsecret aliases, collections, items, metadata, attributes, and
  secret values outside the selected flat entry.
- Attacker: no external attacker is required; an authenticated operator using a
  supported slot-scoped admin write, delete, or rename currently triggers the
  destructive effect.
- Boundary direction: authenticated flat admin API mutation → typed canonical
  `ResourceBlob` → backend `StorageBackend`. Authority to mutate one selected
  entry must not become authority to erase untouched sections of that blob.
- Invariant: `docs/security.md` requires all durable brokered resource material
  to pass through the backend `StorageBackend`, and the canonical resource owns
  env, files, and libsecret together. Within the documented whole-blob
  last-writer-wins model, a slot-scoped mutation preserves every untouched
  section.

## Validation

- `cargo test -p underwrap-backend-store resource_blob`
- `cargo test -p underwrap-backend admin_api::secrets`
- `cargo test -p underwrap-backend`
- `cargo test -p integration-tests flat_secret_mutations_preserve_libsecret`
- `cargo fmt -- --check`
- `cargo clippy --all-targets -p underwrap-backend-store -p underwrap-backend -p integration-tests -- -D warnings`

## Dependencies

- None.
