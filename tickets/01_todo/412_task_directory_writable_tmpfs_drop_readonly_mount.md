# Task: Directory mount always writable inside the namespace

Parent: `410_story_wrapper_buffered_writeback_and_warnings.md`

## Scope

Stop forcing the directory bind mount to read-only when the grant
disallows writes. The namespace-local tmpfs and the bind mounts onto
the target home are always read-write to the target. The server is
now the sole authority for write authorization (rejecting Checkin on
read-only grants per task 403).

### Code changes

In `crates/frontend-ns/src/directory_credential.rs`:

- Remove the `if binding.access == AccessMode::ReadOnly { bind_mount_readonly_to_self(...) }`
  branch.
- Remove the `bind_mount_readonly_to_self` function (and its mounts.rs
  helper if no other caller remains).
- The remount-RO logic for the bind mount is also removed — call
  `bind_mount_dir` with `read_only=false` unconditionally.

In `crates/shared-protocol/`:

- The `AccessMode` value still flows in the wrapper-side scope so
  the wrapper knows whether to even attempt Checkin. Keep the type,
  but the directory mount stops consulting it.

### Loading

Directory contents at namespace start come from the `files` section
of the `ResourceBlob` returned by `Checkout` (task 403), materialized
into the tmpfs staging path. The per-file Channel reads are gone.

### Saving

End-of-run reconciliation against the tmpfs is the responsibility
of task 413, which composes the final `ResourceBlob` from the
tmpfs walk plus the libsecret section from secretsd.

## Acceptance criteria

- [ ] No code path mounts the staging tmpfs or the bind mount as
      read-only.
- [ ] The target can write into the bound directory regardless of
      the grant's `writable` flag.
- [ ] Directory contents at namespace start come from the Checkout
      blob's `files` section — no per-file Channel reads remain.
- [ ] Existing unit tests for directory_credential are updated; new
      tests assert writability of the staging dir under both
      access modes.
- [ ] `cargo test --workspace` green; clippy clean.

## Out of scope

- End-of-run files diff → blob composition — task 413.
- Server-side rejection — already in task 403.
- Warnings on rejection — task 414.
