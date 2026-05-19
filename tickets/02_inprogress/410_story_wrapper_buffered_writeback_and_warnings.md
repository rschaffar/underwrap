# Story: Wrapper-side buffered writeback and read-only warnings

Parent: `epic_atomic_resource_blobs.md`

## Progress so far

The wrapper-resource channel is wired end-to-end and exercises
`Checkout` + `UsageSignal` during real wrapper runs. The two utility
modules the secretsd rewrite needs (`LibsecretSection` canonical
bytes + `LibsecretSection ↔ SecretStore` conversions) are landed and
tested but not yet wired into production code.

- `bb609a2` — daemon-side `wrapper_resource_channel` handler
  (`crates/frontend-underwrapd/src/wrapper_resource_channel.rs`).
  Serves `Checkout` / `Checkin` / `UsageSignal` over an
  authenticated socketpair. 9 unit tests cover canonicality
  enforcement, access-mode gating, and rejection paths.
- `a692d25` — `ExecResourceBinding` (in `shared-protocol/src/exec.rs`)
  and `ReadResourceBinding` (in `shared-protocol/src/message/wrapper.rs`)
  wire types.
- `dda5b4c` — daemon establishes one wrapper-resource channel per
  resource alongside the legacy channels (see
  `crates/frontend-underwrapd/src/wrapper_flow/channels.rs`
  `setup_wrapper_resource_channel`). Channel fd is sent via SCM_RIGHTS
  in the same order as the legacy channels: secretsd (if libsecret),
  then directory fds, then wrapper-resource fds.
- `dda5b4c` — `frontend-exec` receives the new fds into fd 20+i
  (`FIRST_RESOURCE_CHANNEL_FD = 20`) and forwards via `ExecBundle`.
- `171aae9` — `frontend-ns` opens each channel and sends `Checkout`
  at startup (`crates/frontend-ns/src/resource_channel.rs`), capturing
  `(original_canonical, writable, libsecret, files_target_paths)`.
  Channel fds are closed in the child before exec.
- `a55a861` — `frontend-ns` dispatches `UsageSignal` per resource
  after waitpid (`crates/frontend-ns/src/writeback.rs`). Errors go
  to stderr without masking the target's exit status.
- `a3f829f` — `LibsecretSection::to_canonical_bytes()` /
  `LibsecretSection::from_canonical_bytes()` for exchanging the
  section on the ns↔secretsd pipes independently of `ResourceBlob`.
- `b477e94` — `store_from_section()` / `section_from_store()` in
  `crates/frontend-secretsd/src/section_store.rs`. Pure-data
  conversions between protocol `LibsecretSection` (BTreeMap, base64
  SecretBytes) and runtime `SecretStore` (HashMap, raw `Vec<u8>`).
  Round-trip tested. `#![allow(dead_code)]` until wired in.

**Still legacy-driven**: write persistence still flows through the
per-call `SecretsdRequest`/`DirectoryRequest` channels and their
end-of-channel blob flush. `Checkin` is not yet dispatched.

## The remaining cutover

User direction: **no legacy support, no parallel running**. One
focused commit (or a small number of cohesive ones) lands the full
cutover and deletes the legacy paths.

### Architecture being cut to

```
┌─ ns (parent, original PID ns) ─────────────────────────────────┐
│                                                                 │
│  For each resource in scope:                                    │
│    1. Checkout on wrapper-resource channel (fd 20+i)            │
│    2. Decode blob; capture original_canonical + writable        │
│    3. Populate tmpfs from blob.files (if any)                   │
│    4. Bind-mount tmpfs RW to target paths                       │
│    5. If libsecret: write blob.libsecret bytes to init pipe     │
│       (secretsd reads from --init-fd at startup)                │
│                                                                 │
│  Fork into new PID ns:                                          │
│    Child (PID 1): exec target                                   │
│    Parent: waitpid                                              │
│                                                                 │
│  After waitpid (namespace dies, target+secretsd+dbus killed):   │
│    For each resource:                                           │
│      libsecret = read state file at --state-path if exists      │
│                  else use blob.libsecret from Checkout          │
│      files     = walk tmpfs into BTreeMap<path, bytes>          │
│      env       = blob.env from Checkout                         │
│      Compose new ResourceBlob; JCS canonicalize                 │
│      Compare bytes to original_canonical:                       │
│        ==      → UsageSignal                                    │
│        != + w  → Checkin(bytes); on Unauthorized → warn+UsageSignal
│        != + !w → warn + UsageSignal                             │
└─────────────────────────────────────────────────────────────────┘

Inside the namespace (sibling of target):
┌─ secretsd ─────────────────────────────────────────────────────┐
│                                                                 │
│  Args: --bus-address --init-fd N --state-path PATH --ready-fd   │
│                                                                 │
│  Boot:                                                          │
│    1. PR_SET_DUMPABLE=0                                         │
│    2. Read all bytes from --init-fd (one-shot pipe from ns)     │
│    3. LibsecretSection::from_canonical_bytes(bytes)             │
│    4. SecretStore = store_from_section(&section)                │
│    5. Claim org.freedesktop.secrets on the bus                  │
│    6. Signal ready via --ready-fd                               │
│                                                                 │
│  D-Bus handlers (all in-memory mutations):                      │
│    Every Item.SetSecret / Item.Delete / Item.SetAttributes /    │
│    Service.CreateCollection / Service.SetAlias / etc.:          │
│      mutate the in-memory SecretStore                           │
│      section = section_from_store(&store)                       │
│      atomically write section.to_canonical_bytes() to           │
│        --state-path                                             │
│    Item.GetSecret reads from in-memory store (no I/O).          │
│                                                                 │
│  At namespace death: SIGKILL. The state file's content is the   │
│  source of truth for ns; secretsd never sees teardown.          │
└─────────────────────────────────────────────────────────────────┘
```

**Why the on-mutation file write instead of a teardown pipe?** When
the target (PID 1 in the namespace) exits, the kernel SIGKILLs every
other process in the namespace immediately. secretsd has no chance
to respond to a teardown signal. The on-mutation atomic write
(rename-into-place on every mutation) captures state without a
goodbye handshake.

The state file lives at `/tmp/.underwrap-ns-bus/libsecret-state.jcs`
(in the tmpfs that ns mounted for the dbus session bus; visible from
ns's mount namespace after the namespace dies because ns parent
holds that mount namespace open).

### Files to change

**New / heavily rewritten:**

- `crates/frontend-secretsd/src/main.rs` — new args
  (`--init-fd`, `--state-path`; drop `--channel-fd`), read section
  bytes from init-fd at startup, populate `SecretStore` via
  `store_from_section`, pass `state_path: Arc<PathBuf>` into the
  D-Bus services.
- `crates/frontend-secretsd/src/state.rs` (new) — `dump(store, path)`:
  `section_from_store` → `to_canonical_bytes` → atomic write
  (`NamedTempFile::persist` or write-temp-then-rename).
- `crates/frontend-secretsd/src/dbus_service/service.rs` — strip
  the `channel` field; on every mutating handler call
  `state::dump`. `get_secrets` reads from store directly.
- `crates/frontend-secretsd/src/dbus_service/collection.rs` — same
  treatment.
- `crates/frontend-secretsd/src/dbus_service/item.rs` — same;
  `get_secret` reads from `store.get_secret(col, item)`.
- `crates/frontend-secretsd/src/dbus_service.rs` (mod root) —
  remove `channel: Arc<dyn ChannelOps>` from constructors.
- `crates/frontend-secretsd/src/channel.rs` — delete the file
  entirely (or keep just `ItemMeta`/`CollectionMeta` if needed; in
  practice they're not — `section_store.rs` does all the conversion).
- `crates/frontend-ns/src/libsecret.rs` — `setup_libsecret` takes
  `section_bytes: &[u8]` and `state_path: &Path` instead of
  `channel_fd`. Create init pipe via `libc::pipe`; write section
  bytes to write end (and close it so secretsd sees EOF); pass
  read end as `--init-fd`. Pass `--state-path`. Drop fd 4 plumbing.
- `crates/frontend-ns/src/main.rs` — reorder so `Checkout` happens
  before `prepare_mounted_directories`. Pass the libsecret
  resource's `original_canonical` bytes (decode to blob, take
  `.libsecret`, re-serialize via `to_canonical_bytes`) into
  `setup_libsecret`. After waitpid, read state file (if exists)
  and pass to writeback for the compose step.
- `crates/frontend-ns/src/directory_credential.rs` — populate
  staging tmpfs from the resource's `blob.files` (decode
  `original_canonical`) instead of from the legacy directory
  channel. Drop the `DirectoryChannel` type entirely. Keep the
  bind-mount + cleanup logic.
- `crates/frontend-ns/src/writeback.rs` — full decision tree:
  walk tmpfs, read state file, compose blob, JCS-compare, dispatch.
  Helpers: `read_libsecret_state(path) -> Option<LibsecretSection>`,
  `walk_tmpfs(staging) -> BTreeMap<String, SecretBytes>`,
  `compose_blob(env, files, libsecret) -> ResourceBlob`.

**Daemon-side cleanup (delete):**

- `crates/frontend-underwrapd/src/secretsd_channel.rs` — entire file.
- `crates/frontend-underwrapd/src/directory_channel.rs` — entire file.
- `crates/frontend-underwrapd/src/lib.rs` — remove `secretsd_channel`
  and `directory_channel` mod declarations.
- `crates/frontend-underwrapd/src/wrapper_flow/channels.rs` — delete
  `setup_secretsd_channel`, `setup_directory_channel`, and the loops
  that call them. Only `setup_wrapper_resource_channel` remains.
- `crates/frontend-underwrapd/src/wrapper_flow/read.rs` — drop
  `libsecret_access`, drop `file_bindings` from the legacy path.
- `crates/frontend-underwrapd/src/wrapper_flow/mod.rs` — drop
  `libsecret_access` from `ReadResult` and the `establish_scoped_channels`
  call. (Note: this needs threading through several enrollment-error
  early returns too — see how I already added `resource_access` for the
  pattern.)

**Exec-side cleanup:**

- `crates/frontend-exec/src/main.rs` — drop `channel_fd` (libsecret
  legacy fd) handling, drop `directory_bindings`, drop
  `FIRST_DIRECTORY_CHANNEL_FD` and the fd 4 dup2 for libsecret.
  Only `resource_bindings` / `FIRST_RESOURCE_CHANNEL_FD = 20` remains.
- `crates/frontend-exec/src/main.rs` — `request_secret` no longer
  recv's the libsecret/directory fds; only the per-resource fds.

**Protocol-level deletions:**

- `crates/shared-protocol/src/message/secretsd.rs` — entire file.
- `crates/shared-protocol/src/message/directory.rs` — entire file.
- `crates/shared-protocol/src/message.rs` — drop the
  `directory`/`secretsd` mod declarations and re-exports.
- `crates/shared-protocol/src/message/wrapper.rs` — drop
  `ReadDirectoryBinding` (replaced by `ReadResourceBinding`).
- `crates/shared-protocol/src/message/errors.rs` — drop
  `SecretsdErrorCode` and `DirectoryErrorCode`.
- `crates/shared-protocol/src/exec.rs` — drop `ExecDirectoryBinding`
  and the `directories` field of `ExecBundle`.

**Test fixtures to clean up:**

- All `FakeChannel` references in `frontend-secretsd/src/dbus_service/*`
  tests (the channel concept is gone; tests just construct a
  `SecretStore` and test that mutations + `state::dump` produce the
  right state file).
- `frontend-ns/src/env.rs` tests — remove `directories: Vec::new()`
  from `ExecBundle` literals (the field is gone).
- `shared-protocol/src/exec.rs` tests — drop the `directories` field.

**ts_export drift detection:**

- `crates/backend-backendd/tests/ts_export.rs` — no changes expected
  (admin DTOs unaffected by this story), but a `cargo test --workspace`
  run may rewrite `web/src/api/types/index.ts` once if any indirect
  type churn happens.

### Critical security invariants the cutover must preserve

These are from `AGENTS.md` and must not regress:

1. **`PR_SET_DUMPABLE=0`** must be the first action in secretsd's `main`,
   before reading the section bytes from the init pipe.
2. **Wrapper-resource channel fds must never be inherited by the target**.
   Today `resource_channel::close_channel_fds_for_exec` closes them in the
   child before exec. The libsecret init pipe and state-path fd similarly
   must not leak into the target.
3. **`env_clear()` + whitelist for target environment**. Adding
   `--state-path` as an argument to secretsd is fine; do NOT put the
   path or any secret bytes in the target's env.
4. **Atomic file writes**. Use `tempfile::NamedTempFile::persist()` or
   write to `.new` + `rename`. A torn write of the state file would mean
   the next ns read parses garbage.
5. **`UNDERWRAP_SUPPRESS_READONLY_WARNING` must remain visible to the
   wrapper but never propagated to the target** — already enforced via
   `env_clear` + whitelist.

### Suggested commit shape

Two commits (the second is mostly deletions):

1. **"secretsd: in-memory section + atomic tmpfs state file"** —
   secretsd rewrite, ns libsecret.rs rewrite, ns end-of-run
   blob composition + Checkin dispatch, ns directory_credential
   populates from blob, daemon channels.rs drops legacy setups,
   exec drops legacy fd recv/dup2. Workspace tests + clippy green.
2. **"delete legacy IPC types and channels"** — delete the
   files/types listed above. Mechanical.

### Acceptance criteria

- [ ] `secretsd` issues zero requests to the daemon (no
      `SecretsdRequest`/`Response` traffic anywhere; the types are
      deleted).
- [ ] `secretsd` writes its current state to `--state-path` after
      every D-Bus mutation, atomically.
- [ ] `ns` populates the files tmpfs from the Checkout blob's
      `files` section (no `DirectoryChannel` reads).
- [ ] `ns` composes the end-of-run blob from `(env from Checkout,
      files from tmpfs walk, libsecret from state file or Checkout
      fallback)`, JCS-canonicalizes, byte-compares to
      `original_canonical`.
- [ ] Decision tree dispatches correctly: `UsageSignal` /
      `Checkin` / warn + `UsageSignal`. `UNDERWRAP_SUPPRESS_READONLY_WARNING=1`
      silences only the stderr line.
- [ ] `cargo test --workspace` green; `cargo clippy --all-targets
      --workspace -- -D warnings` clean.
- [ ] No file in `crates/shared-protocol/src/message/{secretsd,directory}.rs`.
- [ ] No `secretsd_channel` or `directory_channel` module in
      `crates/frontend-underwrapd/src/lib.rs`.
- [ ] `ReadDirectoryBinding`, `ExecDirectoryBinding`,
      `SecretsdErrorCode`, `DirectoryErrorCode` are not in
      `git grep` output.
- [ ] VM test (`nix build .#checks.x86_64-linux.vm-test`) passes —
      both the libsecret and files round-trips work end-to-end on
      a real NixOS VM.

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
