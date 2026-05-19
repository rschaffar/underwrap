# Task: last_used_at test coverage

Parent: `420_story_last_used_on_pair_records.md`

## Scope

End-to-end coverage that `last_used_at` is bumped under the right
conditions and visible to the SPA.

### Cargo integration tests (`crates/integration-tests/`)

Add to `tests/wrapper_writeback.rs` (introduced in task 415):

- `checkout_bumps_last_used` — fresh pair has `None`; after one
  wrapper invocation that only issues Checkout (and no
  mutations), `last_used_at` is `Some(now)`.
- `checkin_also_bumps_last_used` — a wrapper run that ends with
  a successful Checkin bumps the field; the same `now` resolution
  as the Checkout-only path (i.e. the bump from Checkout is
  already enough; Checkin doesn't re-bump within the same
  second).
- `usage_signal_bumps_last_used` — a no-mutation run with only
  UsageSignal still bumps.
- `rejected_checkin_still_bumps_last_used` — a wrapper run that
  has its Checkin rejected still bumps the field via the
  UsageSignal that follows.
- `one_second_idempotence` — issuing Checkout twice within the
  same second writes the pairs document at most once (observable
  via a write counter in a test fixture or a backend audit hook).

### SPA tests

`web/`:

- Vitest unit test: pair cell renders "never" for null, "<x> min
  ago" for recent, "<absolute>" tooltip.
- Playwright (`web/tests/`): seed a backend with one used and one
  unused pair; load the Wrappers page; assert the rendered text
  on each cell.

## Acceptance criteria

- [ ] All Cargo tests above pass.
- [ ] All Vitest tests pass; `cd web && npm test` green.
- [ ] At least one Playwright case validates the rendered output
      end-to-end.

## Out of scope

- VM test — task 432.
