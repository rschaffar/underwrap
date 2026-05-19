# Story: last_used_at on wrapper/resource pairs

Parent: `epic_atomic_resource_blobs.md`

## Outcome

After this story, every `WrapperResourcePairRecord` carries an optional
`last_used_at: u64` (unix seconds) that is bumped server-side whenever
underwrapd serves a `Checkout` for that pair, and as a side effect of
`Checkin` and `UsageSignal`. The admin matrix view shows the value
("last used 3 minutes ago" / "never") on every pair cell, and the API
exposes it as a typed field consumed by the SPA.

## Tasks

- `421_task_pair_record_last_used_field_and_bump_path.md`
- `422_task_admin_api_and_spa_show_last_used.md`
- `423_task_last_used_test_coverage.md`

## Acceptance criteria

- [ ] `WrapperResourcePairRecord` has `last_used_at: Option<u64>`,
      with a clear "never used" representation.
- [ ] The bump path runs on every wrapper-side request that names a
      resource (Checkout, Checkin, UsageSignal). Idempotent at the
      one-second resolution.
- [ ] Admin pairs listing endpoint includes the field. ts-rs-generated
      TS types pick it up automatically.
- [ ] The Wrappers/matrix page renders the value with a humanized
      relative-time label and an absolute timestamp on hover.
- [ ] `cargo test --workspace` + `cd web && npm test` green; clippy
      and SPA lints clean.
