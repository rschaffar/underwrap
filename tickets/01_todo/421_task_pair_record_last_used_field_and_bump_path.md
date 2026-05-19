# Task: WrapperResourcePairRecord.last_used_at field and bump path

Parent: `420_story_last_used_on_pair_records.md`

## Scope

Add a `last_used_at: Option<u64>` field (unix seconds) to
`WrapperResourcePairRecord` and wire the daemon to bump it on every
wrapper-side request that resolves to a specific pair.

### Model change

```rust
pub struct WrapperResourcePairRecord {
    pub wrapper: String,
    pub resource: String,
    pub created_by: EntityCreator,
    pub created_at: u64,
    pub last_used_at: Option<u64>,   // None = never used since record creation
}
```

`Option<u64>` keeps "never used" distinct from "used exactly at the
Unix epoch". ts-rs serializes this naturally as `number | null`.

The pair record lives in `__underwrap/pairs/document` (already in
the entity model). Bumping last_used_at is a load → mutate → store
of that document.

### Bump call site

In the new wrapper-resource channel handler from task 403, after
the authz / scope check succeeds (i.e. the request was *served*,
not rejected), bump `last_used_at` for the resolved
`(wrapper_instance, wrapper_logical_name, resource_id)` pair to
`now`.

Triggered by: Checkout, Checkin (only if accepted), UsageSignal.

### Idempotence and write amplification

- Resolution is one-second: if `last_used_at == now`, skip the
  document write. Cheap guard avoids hammering the pairs document
  in tight test loops.
- The pair document write is already protected by the existing
  backend write lock — no new locking concerns.
- For multiple resources used by one wrapper run, a single
  document load → multi-field mutate → one store is acceptable.

### EntityCreator-vs-current-instance subtlety

`last_used_at` is global to the pair — it records "this pair has
been used at most-recently `t`", regardless of which client
instance ran the wrapper. The current instance identity is only
used to look up the pair; we don't need a per-instance histogram
in this epic.

## Acceptance criteria

- [ ] `WrapperResourcePairRecord` has the field; ts-rs surface
      updates.
- [ ] Bump call site present in the wrapper-resource channel
      handler; covered by unit tests including the
      one-second-idempotence guard.
- [ ] Unit tests on the pairs document: load → bump → store →
      load round-trips the field; pre-existing records without
      the field deserialize as `None` (use `#[serde(default)]`).
- [ ] `cargo test --workspace` green; clippy clean.

## Out of scope

- Admin API surface — task 422.
- SPA rendering — task 422.
- Coverage — task 423.
