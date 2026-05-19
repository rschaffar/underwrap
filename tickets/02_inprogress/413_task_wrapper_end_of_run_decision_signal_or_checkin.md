# Task: End-of-run decision — Checkin / UsageSignal / quiet drop

Parent: `410_story_wrapper_buffered_writeback_and_warnings.md`

## Scope

`frontend-ns` is the sole end-of-run decider. After `waitpid` returns,
it composes the final `ResourceBlob` from the current namespace state,
JCS-canonicalizes it, byte-compares to the bytes captured at Checkout,
and dispatches one of three actions per resource.

### Captured-at-Checkout state

`frontend-ns` keeps for each resource:

- `resource_id: String`
- `original_canonical: Vec<u8>` — the bytes returned by Checkout.
- `writable: bool` — the writability flag from Checkout.

### End-of-run composition

For each resource in scope:

1. Build a fresh `ResourceBlob` from current state:
   - `env`: unchanged from Checkout (env section is read-only from
     the target — no in-process mutation path). Pass through.
   - `files`: walk the tmpfs staging dir (existing
     `collect_managed_files` logic), populate
     `BTreeMap<String, SecretBytes>` from file contents.
   - `libsecret`: pull the in-memory `LibsecretSection` from
     secretsd via the secretsd→frontend-ns transport added in
     task 411 (single round-trip teardown signal).
2. `new_canonical = blob.to_canonical_bytes()`.
3. Decision:
   ```
   if new_canonical == original_canonical:
       send UsageSignal
   else if writable:
       send Checkin(new_canonical)
       on Checkin Error::Unauthorized (server-side race):
           emit warning (task 414), then send UsageSignal
   else:
       emit warning (task 414), then send UsageSignal
   ```

### Channel ordering

The end-of-run requests go on the same channel fd 4. Because
secretsd no longer sends per-call traffic during the run, the
channel is quiescent until teardown — no contention.

After the final request response is received, `frontend-ns` closes
fd 4 and proceeds with the existing cleanup path (drop caps, etc.).

### Crash handling

If the target was killed (non-zero exit), still attempt the
end-of-run decision. The user's stated model is last-write-wins;
crashed-but-mutated state still gets a Checkin attempt.

If `frontend-ns` itself is SIGKILLed, nothing happens — same
crash semantics as today.

## Acceptance criteria

- [ ] One file in `frontend-ns` (e.g. `writeback.rs`) holds the
      composition and dispatch logic with the decision tree shown
      above.
- [ ] The Checkin / UsageSignal call happens exactly once per
      resource per wrapper invocation.
- [ ] On a clean run with no mutations: only UsageSignal is sent;
      no Checkin.
- [ ] On a clean run with mutations and a writable grant: exactly
      one Checkin is sent with the canonical bytes.
- [ ] On a clean run with mutations and a non-writable grant: no
      Checkin attempt, warning emitted (task 414), UsageSignal sent.
- [ ] Unit tests in `frontend-ns` exercise the decision tree with a
      fake transport and stub secretsd handoff.
- [ ] `cargo test --workspace` green; clippy clean.

## Out of scope

- The actual journal/stderr warning emission — task 414.
- secretsd's half of the teardown transport — task 411.
- Integration / VM coverage — task 415 and 432.
