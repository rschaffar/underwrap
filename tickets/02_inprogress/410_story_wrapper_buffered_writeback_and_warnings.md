# Story: Wrapper-side buffered writeback and read-only warnings

Parent: `epic_atomic_resource_blobs.md`

## Progress

The wrapper-resource channel is now wired end-to-end and exercises
`Checkout` + `UsageSignal` during real wrapper runs:

- `bb609a2` — daemon-side `wrapper_resource_channel` handler.
- `a692d25` — `ExecResourceBinding` / `ReadResourceBinding` wire
  types added to `shared-protocol`.
- `dda5b4c` — daemon establishes wrapper-resource channels per
  resource and ships fds via SCM_RIGHTS; `frontend-exec` receives
  them into fd 20+i and forwards via `ExecBundle`.
- `171aae9` — `frontend-ns` opens each channel and sends `Checkout`
  at startup, capturing `(original_canonical, writable)` plus
  libsecret/files metadata. Channel fds are closed in the child
  before exec.
- `a55a861` — `frontend-ns` dispatches `UsageSignal` per resource
  after waitpid. Errors go to stderr without masking the target's
  exit status.

**Still legacy-driven**: actual write persistence still flows through
the per-call `SecretsdRequest`/`DirectoryRequest` channels and their
end-of-channel blob flush. `Checkin` is not yet dispatched.

### Remaining cutover (tasks 4 + 5 finish + 6)

The cleanest path is one atomic cutover commit that:

1. Rewrites `frontend-secretsd` to receive its `LibsecretSection`
   bytes from a pipe fed by `frontend-ns` at startup, holds the
   section in memory, mutates from D-Bus without any per-call IPC,
   and writes its current section back to `frontend-ns` over a
   teardown pipe on signal. Removes the daemon-channel arg
   (`--channel-fd`) entirely.
2. Has `frontend-ns` compose the final `ResourceBlob` at end-of-run
   from the captured Checkout state (env), the tmpfs files tree, and
   secretsd's returned `LibsecretSection`. JCS-canonicalize, byte-
   compare against `original_canonical`, dispatch one of:
   - equal → `UsageSignal` (already wired);
   - changed + writable → `Checkin(new_canonical)`;
   - changed + not writable, or `Checkin` returns `Unauthorized` →
     stderr+journal warning + `UsageSignal`.
3. Removes `SecretsdRequest`/`Response`, `DirectoryRequest`/`Response`
   types and the corresponding daemon channel modules.

This is the work blocked by needing a focused design pass on the
ns↔secretsd pipe protocol. The wire-level foundation is in.

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
