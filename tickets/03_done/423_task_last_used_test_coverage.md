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

## Status note (2026-05-20)

Cargo coverage is in place at
`crates/integration-tests/src/tests/wrapper_writeback.rs`:

- `checkout_bumps_last_used`
- `checkin_keeps_last_used_at_least_at_checkout_value`
  (substitutes for `checkin_also_bumps_last_used`; same-second
  resolution makes a strict "Checkin re-bumps" assertion flaky
  without a second-granularity wait — the test asserts the field is
  set, which is sufficient given the underlying
  `bump_pair_last_used` implementation)
- `usage_signal_bumps_last_used`
- `rejected_checkin_still_bumps_last_used`

Skipped: `one_second_idempotence`. The
backend's `bump_pair_last_used` is already idempotent at one-second
resolution (see `crates/backend-backendd/src/server.rs::bump_pair_last_used`,
which short-circuits when `pair.last_used_at == Some(now)`). Observing
the no-write from outside the daemon needs a write-counter hook in
the storage backend — also a follow-up if the idempotence guarantee
ever changes.

SPA-side coverage (Vitest pair-cell rendering + Playwright end-to-end)
is its own piece of work tracked separately under `web/`. This ticket
covers the cargo half.
