# Story: Wrapper-side buffered writeback and read-only warnings

Parent: `epic_atomic_resource_blobs.md`

## Progress

- **Foundation landed (commit `bb609a2`)**: daemon-side
  `wrapper_resource_channel` module serves `Checkout` / `Checkin` /
  `UsageSignal` over an authenticated socketpair, with JCS
  canonicality enforcement on `Checkin` and access-mode gating. 9 unit
  tests cover all three verbs and the rejection paths. **No consumer
  yet** — wrapper chain still uses the legacy per-call secretsd /
  directory channels.

The remaining tasks below need a cohesive design pass on the
secretsd↔frontend-ns inter-process protocol before implementation:
the new channel is per-resource, but the legacy channels are
per-slot, so a parallel-with-legacy rollout is awkward and the right
move is a single cutover commit (tasks 2–6 together).

## Outcome

After this story, every wrapper invocation pulls its resource blob once
at namespace start, runs the target against an entirely local buffer
(in-process for libsecret, tmpfs for files), and decides at end of run
whether to send a `Checkin`, a `UsageSignal`, or nothing-with-a-warning.
The directory mount inside the namespace is always read-write to the
target; the server is the only gate on actual persistence.

## Tasks

- `411_task_secretsd_full_blob_residency_and_buffering.md`
- `412_task_directory_writable_tmpfs_drop_readonly_mount.md`
- `413_task_wrapper_end_of_run_decision_signal_or_checkin.md`
- `414_task_journal_stderr_warning_with_quiet_env_var.md`
- `415_task_wrapper_writeback_test_coverage.md`

## Acceptance criteria

- [ ] secretsd reads the resource's libsecret section out of the
      Checkout blob and serves every D-Bus call from the in-memory
      copy. No per-call traffic to underwrapd during the run.
- [ ] All directory bindings mount read-write inside the namespace
      regardless of `AccessMode`. The `bind_mount_readonly_to_self`
      call site is gone or repurposed.
- [ ] On namespace teardown the wrapper produces JCS-canonical bytes
      of the current state, byte-compares to the bytes captured at
      Checkout, and:
      - sends `UsageSignal` if equal;
      - sends `Checkin` if changed and writable;
      - drops the change locally if changed and not writable, after
        emitting one structured journal line and one stderr line
        (unless `UNDERWRAP_SUPPRESS_READONLY_WARNING=1`).
- [ ] The stderr suppression env var is honored by the wrapper but
      never propagated to the target process (env whitelist enforced).
- [ ] Server still rejects a `Checkin` against a read-only grant; on
      rejection the wrapper emits the same journal + stderr warning
      and exits cleanly.
- [ ] `cargo test --workspace` green; clippy clean.
