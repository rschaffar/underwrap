# Task: Give the uid-X target read/write access to the caller's $HOME and $XDG_RUNTIME_DIR via idmapped mounts

Parent: `2600_story_target_uid_isolation`
Depends on: `2630_task_ns_setresuid_target_uid`

## Problem

After 2630 the target runs as uid X, so it can no longer touch the caller's
files (`/home/robert`, owned uid 1700, mode 0700) or connect to the caller's
sockets (`$XDG_RUNTIME_DIR/...`, owned 1700) — `connect()` checks write
permission against the caller's fsuid, now X. We need the target to read **and
write** the caller's files *as the caller*, with writes landing on disk owned by
the caller, without changing any on-disk ownership/permissions.

This is exactly the kernel's "portable home directory" idmapped-mount use case
(`Documentation/filesystems/idmappings.rst`), and how `podman --userns=keep-id`
+ `:idmap` volumes work.

## Design

In the child's private mount namespace (already created via
`unshare(CLONE_NEWNS)` in `prepare_namespace`), set up **idmapped bind mounts**
of the caller's `$HOME` and `$XDG_RUNTIME_DIR` whose idmap translates the
caller's on-disk uid/gid (1700) ⇄ the target's kernel uid/gid (X).

- Read path: on-disk owner 1700 → presented to the target (fsuid X) as **owner
  X** → owner-rwx applies → full read/write.
- Write path: files created by the target (uid X) reverse-map → written to disk
  as **uid 1700** (the caller). So `~/.aws/cache` etc. end up owned by the
  caller, exactly as if they'd run it themselves.
- On-disk ownership/permissions are **never** modified; the translation is
  mount-local and private to the namespace.

### Mechanics (the intricate part — crib from crun/bwrap)

1. Build the idmap **userns object**: fork a short-lived helper that
   `unshare(CLONE_NEWUSER)`, write its `uid_map`/`gid_map` describing the
   `1700 ⇄ X` translation, and keep its `/proc/<pid>/ns/user` fd. This userns
   exists **only** to carry the mapping for `mount_setattr`; it is distinct from
   the process userns added in 2650.
2. For each path (`$HOME`, `$XDG_RUNTIME_DIR`): create a bind mount
   (`mount(src, dst, NULL, MS_BIND|MS_REC, NULL)`), then
   `mount_setattr(dst, AT_..., &{ attr_set: MOUNT_ATTR_IDMAP, userns_fd })`.
3. Requires `CAP_SYS_ADMIN` **in the init userns** to idmap a host filesystem —
   `underwrap-ns` has it (file cap). Do this **before** the `setresuid`/`drop_caps`
   in 2630 (insert at the marked point in 2630's ordering), then drop caps.
4. Mounts must be set up before `setresuid` to X and before entering the process
   userns (2650). The bind targets can be the real paths (we're in a private
   mount ns, so re-binding `/home/robert` over itself is invisible to the host).

### Filesystem support — verify per host

idmapped mounts need fs support: ext4/xfs/btrfs/tmpfs/f2fs/overlayfs/FAT yes;
**ZFS only ≥ OpenZFS 2.2.0**. Detect at setup and fail with a clear error if the
backing fs of `$HOME` doesn't support `MOUNT_ATTR_IDMAP` (don't silently fall
back to an inaccessible home). `$XDG_RUNTIME_DIR` is tmpfs (supported).

### Scope of mapping

- Map the caller's primary uid (1700) and primary gid. Supplementary-group
  access is out of scope (note it).
- Only `$HOME` and `$XDG_RUNTIME_DIR` are idmapped. Other paths the target needs
  (extra drives, separate mounts) are not covered — document this; add more
  idmapped binds later if a real wrapper needs them.

### Module surface

- The set of idmapped paths is derivable (`$HOME` from the resolved target
  identity, `$XDG_RUNTIME_DIR` from `xdg_runtime_dir_base`/uid). Add an optional
  per-instance/per-wrapper `idmapPaths` list for extra paths, default
  `[ "$HOME", "$XDG_RUNTIME_DIR" ]`. Keep it minimal.

## Acceptance

- New VM fixture `test-iso-home`: a script (run via the isolated instance) that
  (a) reads a pre-seeded file in `$HOME` owned by `testuser`, (b) writes a new
  file under `$HOME`, (c) prints `id -u` and the `stat` owner of what it sees.
  Assertions in `test.py`:
  - the target reads the pre-seeded file successfully;
  - after the run, the **on-disk** new file is owned by `testuser` (1099), not by
    the dedicated uid X (checked as root);
  - `/home/<testuser>` on the host is still owned by `testuser` with unchanged
    mode (no chown leaked out).
- Socket reachability: with `$XDG_RUNTIME_DIR` idmapped, a fixture can `connect()`
  to a `testuser`-owned `0700` unix socket placed in the runtime dir (prove
  EACCES becomes success). (SSH-agent-style; D-Bus peer-cred is out of scope.)
- The 2630 protection assertions still hold (caller still can't read the target's
  `environ`/`mem`).
- Graceful, explicit failure when `$HOME`'s filesystem lacks idmapped-mount
  support.
- `cargo test -p underwrap-ns`, `cargo fmt --check`, `nix build .#<vm test attr>`
  green.

## Out of scope

- `getuid()==caller` relabel (2650) — at this ticket the target still sees
  `getuid()==X` and files display as the dedicated user's name (access works;
  only the displayed identity differs). 2650 fixes the cosmetics.
- Supplementary groups; real session D-Bus; per-invocation uids.
