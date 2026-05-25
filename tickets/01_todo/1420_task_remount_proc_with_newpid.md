# Task: Always remount `/proc` when `CLONE_NEWPID` is set

Part of: `epic_frontend_security_hardening_2026-05`.

## Problem

`crates/frontend-ns/src/main.rs:prepare_namespace` unshares
`CLONE_NEWPID` unconditionally, but `CLONE_NEWNS` (and the subsequent
`/proc` remount inside `setup_mount_namespace`) only happen when
`libsecret_enabled || has_files`. In env-only mode there is no mount
namespace and no `/proc` remount at all.

Consequence: `/proc` is bound to the PID namespace of the *mounter*,
not of the reader. A new PID namespace without a fresh `/proc`
inherits the host's `/proc`. Inside the new namespace, the target
can read `/proc/<host-pid>/*` for any host process. Yama
`ptrace_scope=2` still gates the sensitive entries (`mem`,
`environ`, `fd/*`), so the "Guarantee" in `docs/security.md` is
preserved — but the framing in `docs/security.md` §"What the PID
namespace contributes" claims:

> giving the target a separate PID space
> making process discovery less direct from inside the namespace

The first is only true at the syscall level. The second is false in
env-only mode: process discovery via `/proc` works exactly as on the
host.

The code change is cheap and aligns the implementation with the
existing framing.

## What to do

### Code (`crates/frontend-ns/`)

1. In `prepare_namespace`, unshare `CLONE_NEWPID | CLONE_NEWNS`
   unconditionally. The `libsecret_enabled || has_files` branch
   becomes unnecessary; collapse it.

2. After the unshare, also unconditionally call
   `mount("none", "/", NULL, MS_PRIVATE|MS_REC, NULL)` so the new
   mount namespace doesn't propagate back. (Today this lives in
   `setup_directory_mount_namespace_base`; lift it into
   `prepare_namespace` and keep `setup_directory_mount_namespace_base`
   as just the staging-root tmpfs setup.)

3. Move the `/proc` remount out of `setup_mount_namespace` (which
   stays libsecret-specific for the bus dir + real-bus hide) into a
   small `remount_proc()` helper called from `run_child`
   unconditionally — it must happen after the `fork_into_namespace`
   because mounting a new `proc` requires the mount to happen in a
   process that is *inside* the new PID namespace (otherwise the
   new mount still binds to the original PID ns).

4. Keep `setup_mount_namespace`'s libsecret-specific work (bus-dir
   tmpfs, hide `/run/user/<uid>/bus`) gated on `libsecret_enabled`.

5. Verify the file-credential staging tmpfs setup in
   `setup_directory_mount_namespace_base` still works when called
   unconditionally — should be fine because it only mounts when
   `mount_directory_staging` is true.

### Doc (`docs/security.md`)

1. Update §"What the PID namespace contributes" to reflect the new
   reality: `/proc` is fresh-mounted in every mode that uses a PID
   namespace, so process discovery from inside the namespace really
   is limited to namespace-visible PIDs in every mode.

2. Remove the parenthetical "combining well with a fresh `/proc`
   mount in libsecret mode" — it now applies to all modes.

## Acceptance

- `cargo test --workspace`
- `cargo clippy --all-targets --workspace -- -D warnings`
- New VM-test assertion (or extend an existing one in
  `nix/tests/test.py`): under an env-only wrapper, the target sees
  exactly its namespace PIDs in `/proc` — `ls /proc | grep -E '^[0-9]+'`
  returns only the namespace's local PIDs, not host PIDs.
- VM test green.
- The "What the PID namespace contributes" section reads truthfully
  for env, files, and libsecret modes uniformly.
