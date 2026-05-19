# Story: Resource blob storage layout and checkout/checkin protocol

Parent: `epic_atomic_resource_blobs.md`

## Outcome

After this story, a resource on the backend is one storage key whose
value is a JCS-canonical JSON document carrying its env values, files
tree, and libsecret keyring. The wrapper-side IPC channel speaks
`Checkout` / `Checkin` / `UsageSignal` instead of per-key libsecret and
directory verbs. Admin HTTPS endpoints expose the new shape.
Wrapper-side behavior is the next story; this one is foundation only.

## Tasks

- `401_task_resource_blob_schema_and_jcs_canonicalization.md`
- `402_task_storage_layout_one_key_per_resource.md`
- `403_task_protocol_checkout_checkin_usage_signal.md`
- `404_task_backend_admin_api_resource_blob_get_replace.md`
- `405_task_resource_blob_protocol_test_coverage.md`

## Acceptance criteria

- [ ] `ResourceBlob` type lives in `shared-protocol`, integer / string
      / bool fields only, `BTreeMap` / `BTreeSet` everywhere, with
      sections for `env`, `files`, `libsecret`.
- [ ] `serde_jcs::to_vec(&blob)` is deterministic across a
      serialize → deserialize → serialize round-trip.
- [ ] `StorageBackend` stores and reads the blob as one opaque key per
      resource; the per-slot/per-item key shape is gone.
- [ ] `SecretsdRequest::{Read,Write,List,Delete}` and
      `DirectoryRequest::{ReadFile,WriteFile,DeleteFile,ListFiles}`
      are removed. The new verbs are `Checkout`, `Checkin`,
      `UsageSignal` on a single wrapper-side IPC channel.
- [ ] Backend admin endpoints can read and replace a resource blob;
      replacement is atomic at the storage layer.
- [ ] `cargo test --workspace` green; clippy clean.
