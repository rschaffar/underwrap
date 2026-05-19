# Task: ResourceBlob schema and JCS canonicalization

Parent: `400_story_resource_blob_storage_and_protocol.md`

## Scope

Add a new `ResourceBlob` type in `shared-protocol` that carries the
entire content of one backend resource. Pin the schema so that
`serde_jcs::to_vec(&blob)` is deterministic and round-trip stable.

### Shape

```rust
pub struct ResourceBlob {
    pub schema_version: u32,        // start at 1; reject unknown
    pub env: BTreeMap<String, SecretBytes>,
    pub files: BTreeMap<String, SecretBytes>,
    pub libsecret: LibsecretSection,
}

pub struct LibsecretSection {
    pub aliases: BTreeMap<String, String>,        // alias -> collection_id
    pub collections: BTreeMap<String, Collection>,
}

pub struct Collection {
    pub label: String,
    pub created: u64,
    pub modified: u64,
    pub items: BTreeMap<String, Item>,
}

pub struct Item {
    pub label: String,
    pub content_type: String,
    pub created: u64,
    pub modified: u64,
    pub attributes: BTreeMap<String, String>,
    pub secret: SecretBytes,
}
```

Rules:
- Every map and set is a `BTreeMap` / `BTreeSet`. No `HashMap` /
  `HashSet` anywhere on this type, recursively.
- No `f32` / `f64`. Timestamps stay `u64` seconds.
- `SecretBytes` already exists; the JCS form is the existing
  base64-string representation.
- No `serde(untagged)` enums in the schema — JCS handles tagged enums
  cleanly, untagged ones are an easy source of nondeterminism.

### API

```rust
impl ResourceBlob {
    pub fn empty() -> Self;
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>>;   // serde_jcs
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self>;
}
```

`to_canonical_bytes` is what both the wrapper-side and admin-side
writers call before pushing. `from_canonical_bytes` rejects on unknown
schema version with a clear error.

Add `serde_jcs` as a workspace dependency.

## Acceptance criteria

- [ ] `ResourceBlob` type lands in `crates/shared-protocol/`, derives
      `Serialize` / `Deserialize` / `Clone` / `PartialEq` / `Eq`.
- [ ] `to_canonical_bytes` and `from_canonical_bytes` exist and are
      the only documented way to (de)serialize for storage and the
      wire.
- [ ] `serde_jcs` dependency added to the workspace, no version
      conflict with existing `serde_json`.
- [ ] Table-driven unit tests cover: empty blob; populated env;
      populated files; populated libsecret with multiple collections
      and items; unicode strings; large `u64` near `u64::MAX`;
      schema_version round trip; unknown schema_version is rejected;
      serialize → deserialize → serialize is byte-equal.
- [ ] `cargo fmt`, `cargo clippy --all-targets --workspace -- -D warnings`,
      `cargo test --workspace` all green.

## Out of scope

- Storing or transmitting the blob — task 402 and 403.
- Changes to `EncryptedBackend` / `PlainBackend` internals beyond
  consuming the new opaque blob bytes — task 402.
