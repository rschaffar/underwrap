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

> **Placement:** the idmap setup + the files-slot-overlay ordering slot into the
> **canonical ordering in 2630** ("the SPINE") at the `[2640]` tags (in the
> *parent, pre-fork* phase — doing it there, before `prepare_files_mounts`, is how
> the files-slot binds end up overlaying the idmapped home rather than being
> shadowed; see the ⚠ ordering section below).

Set up **idmapped bind mounts**
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

**The map direction (one map does read + writeback).** The idmap userns map
written is `uid_map = "1700 X 1"` (inside-id `1700`, outside-id `X`, count `1`)
and `gid_map = "<callergid> Xgid 1"`. A mount idmap built from this does
`map_id_down(1700) = X`, so an on-disk-1700 file is presented to the fsuid-X
target as **owner X** → owner-rwx → full read/write (`fs/mnt_idmapping.c:96`,
`make_vfsuid`); and `map_id_up(X) = 1700`, so files the target creates are written
back **owned by 1700** (`:159`, `from_vfsuid`). The permission check is
kuid-based, so this composes with 2650's process userns unchanged (target fsuid
kuid X == file's presented kuid X → owner). Verified against `kernel/ptrace.c`
and `fs/mnt_idmapping.c` on v7.1-rc6.

1. **Build the idmap userns object carrying `1700 ⇄ X`.** Note `"1700 X 1"` is
   **not** a single self-map for a writer whose euid is 1700: the kernel's
   unprivileged self-map requires the mapped parent id to equal the *writer's own*
   euid (`new_idmap_permitted`, `kernel/user_namespace.c:1184-1196`), and a
   process that has already `unshare`d `CLONE_NEWUSER` has **lost** its caps over
   the parent (init) userns, so it cannot write this map itself (`:1207` fails).
   Two correct options:
   - **(a) parent-writes-child (preferred, matches crun):** the cap-holding
     `underwrap-ns` child (still `CAP_SETUID`/`CAP_SETGID` in init) forks a helper
     that `unshare`s `CLONE_NEWUSER` and blocks; the **parent** writes
     `/proc/<helper>/{uid,gid}_map` (it has the caps over init → `:1207` passes),
     then opens `/proc/<helper>/ns/user`. Set `/proc/<helper>/setgroups` to `deny`
     before writing gid_map. The open `ns/user` fd pins the userns alive, so kill
     and `waitpid`-reap the helper afterward — otherwise it lingers in the PID
     namespace. (Per the canonical
     spine in 2630, this whole idmap-userns + `open_tree`/`mount_setattr`/
     `move_mount` dance runs in the **parent, pre-fork** — the mount ns is already
     shared, caps are held there, `open_tree` traverses the real on-disk-1700 home
     as its owner (fsuid 1700), and it avoids a nested fork inside the new PID
     namespace. The files-slot binds are then layered on top of this idmapped home
     **from the child**, after its `setresuid` to X (2630) — later in time, so they
     overlay the idmapped home rather than being shadowed by it. Do the idmap in the
     parent, not in `run_child`; do the binds in `run_child`, not the parent.)
   - **(b) helper-becomes-X-first:** the helper `setresgid`/`setresuid` to X
     first, *then* `unshare`s, *then* self-writes `"1700 X 1"` — now a genuine
     self-map (mapped parent id X == helper euid X), permitted unprivileged
     (gid_map still needs `setgroups`=`deny` first, `:1193`).

   Keep the resulting `/proc/<pid>/ns/user` fd. This userns exists **only** to
   carry the mapping for `mount_setattr`; it is distinct from the process userns
   in 2650.
2. **For each path, idmap a *detached* mount and move it into place — you cannot
   idmap an attached bind mount.** `can_idmap_mount` requires the target mount be
   anonymous (`is_anon_ns`, `fs/namespace.c:4823`); a plain
   `mount(src, dst, MS_BIND)` produces an *attached* mount, so `mount_setattr`
   returns `EINVAL`. The working sequence (what crun/bwrap actually do):
   - `open_tree(AT_FDCWD, src, OPEN_TREE_CLONE | OPEN_TREE_CLOEXEC)` → a detached
     clone of the subtree at `src` (this *is* the bind; do **not** `mount(MS_BIND)`).
   - `mount_setattr(fd, "", AT_EMPTY_PATH, &{ attr_set: MOUNT_ATTR_IDMAP |
     **MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV**, userns_fd }, size)` on the
     **detached** fd (add `AT_RECURSIVE` if you cloned recursively). The
     `NOSUID|NODEV` is **mandatory**, not cosmetic — see the setuid note below.
   - `move_mount(fd, "", AT_FDCWD, dst, MOVE_MOUNT_F_EMPTY_PATH)` to attach it at
     the target path.
   - **Resolve the bind *source* race-safely** (runc CVE-2025-31133 / 52565
     class): `open_tree` the source via an `openat2(RESOLVE_NO_SYMLINKS |
     RESOLVE_NO_MAGICLINKS)` fd (or verify the resolved inode), so a symlink at
     `$HOME`/`$XDG_RUNTIME_DIR` cannot redirect the clone to an unintended tree.
   - **Close the idmap `userns_fd` (and reap the helper) before the target exec**
     — it is needed only for `mount_setattr`; leaking it into the target is a
     Leaky-Vessels-class fd leak (see 2645's `close_range` sweep).
3. **Capabilities.** `mount_setattr(MOUNT_ATTR_IDMAP)` needs `CAP_SYS_ADMIN` over
   the **filesystem's** userns (= init for a host fs — `fs/namespace.c:4819`) and
   over the idmap userns (held as its creator/ancestor — `:5030`); the idmap
   userns must not be init itself (`:5026`). `underwrap-ns` has `CAP_SYS_ADMIN` in
   init (file cap). Do all of this **before** the `setresuid`/`drop_caps` in 2630
   (at the marked insertion point), then drop caps.
4. **Ordering.** Idmapped mounts are set up before `setresuid` to X and before
   the process userns (2650). Targets are the real paths — we're in the private
   mount ns, so `move_mount`-ing an idmapped clone over `/home/robert` is
   invisible to the host.
5. **Interaction with the libsecret real-bus hide.** `$XDG_RUNTIME_DIR` is
   `/run/user/<uid>`, and `setup_mount_namespace` hides the real session bus by
   bind-mounting `/dev/null` over `/run/user/<uid>/bus` (`mounts.rs`). An idmapped
   `move_mount` of the whole runtime dir would re-expose that bus. It is mooted by
   dbus peer-cred (the real bus rejects a uid-X peer), but to keep the hide,
   either re-apply the `/dev/null` bind **after** the idmapped `move_mount`, or
   idmap a narrower path. Call this out where the two orderings meet.

### setuid / device safety on the idmapped mount (`MOUNT_ATTR_NOSUID|NODEV`)

The idmapped mount is a **privilege-translation boundary** (target-created files
become caller-owned, caller-owned files appear target-owned), and such boundaries
are a classic escalation primitive — see overlayfs copy-up **CVE-2023-0386**,
where a setuid bit / `security.capability` surviving a cross-mount move yielded
root. Concretely here:

- *Inbound:* a pre-existing setuid file (or fscap'd binary) in the user's `$HOME`
  is presented to the uid-X target as owner-X with the setuid bit intact — the
  target could exec it.
- *Outbound:* the target can `chmod u+s` a file it creates (no capability needed
  to set the bit); via the idmap that file lands on disk **owned by the caller
  (1700) with the setuid bit** — a setuid-1700 binary planted in the user's home.

`MOUNT_ATTR_NOSUID` makes setuid/setgid/fscaps **inert when executed via the
idmapped mount**, neutralizing the inbound vector cleanly. `MOUNT_ATTR_NODEV`
blocks device-node honoring (the target has no `CAP_MKNOD`, but free hygiene).
**Residual, documented:** `NOSUID` on *our* mount does not stop the user from
later executing a target-planted setuid-1700 file via their *own* (non-nosuid)
view of `$HOME` — that requires the user to run an attacker-planted binary
(social), and fully closing it would mean stripping mode bits on writeback
(out of scope). Note it in `docs/security.md`. Do **not** add `MOUNT_ATTR_NOEXEC`
— targets legitimately run binaries from `~/.local/bin` etc.

### Filesystem support — verify per host

idmapped mounts need fs support (`FS_ALLOW_IDMAP`, `fs/namespace.c:4811`):
ext4/xfs/btrfs/tmpfs/f2fs/overlayfs/FAT yes; **ZFS only ≥ OpenZFS 2.2.0**;
**9p/NFS no**. Detect at setup and fail with a clear error if the backing fs of
`$HOME` doesn't support `MOUNT_ATTR_IDMAP` (don't silently fall back to an
inaccessible home). `$XDG_RUNTIME_DIR` is tmpfs (supported). **Note for the VM
test:** the NixOS test framework backs the host store share with 9p, so pin
`testuser`'s `$HOME` onto an idmap-capable fs (tmpfs or an ext4 image) for the
2660 home-rw assertions to be deterministic. (Syscall floor: `mount_setattr` /
idmapped mounts need kernel ≥5.12, `openat2(RESOLVE_*)` ≥5.6 — both well under
underwrap's existing ≥6.5 floor pinned by `hardenTargetHost`; no new minimum.)

### Scope of mapping

- Map the caller's primary uid (1700) and primary gid. Supplementary-group
  access is out of scope (note it).
- Only `$HOME` and `$XDG_RUNTIME_DIR` are idmapped. Other paths the target needs
  (extra drives, separate mounts, user-private files outside `$HOME`) are not
  covered — document this; the target gets only "other" perms on a user-private
  file outside an idmapped path. Add more entries via `idmapPaths`. **Blanket
  access (idmapping `/` recursively) is deliberately not done** — it is invasive
  and fragile (every submount, procfs/sysfs restrictions); per-path is the model.
- The **files-slot staging tmpfs/binds and the libsecret bus tmpfs are handled in
  2630, not here** — they are target-facing scratch surfaces created by
  `underwrap-ns` itself, made usable by uid X via 2630's *decided* staging-access
  design: created `uid=X` and mounted/materialized/bound **in the child after the
  `setresuid`, at natural fsuid X** (retained `CAP_SYS_ADMIN`); the supervisor
  reads them only inside its single writeback `setfsuid(X)` window. Do **not**
  idmap them — they are simply owned by X. Only the **caller's own** files
  (`$HOME`, `$XDG_RUNTIME_DIR`) are idmapped here.

### ⚠ Ordering vs. files-slot projections under `$HOME` (correctness bug if missed)

Files-slot binds target `<target_home>/<mount_path>` — i.e. paths **under `$HOME`**
(`directory_credential.rs:33,95`: `~/.aws`, `~/.config/gh`, …) — and *today*
(unisolated) they are established in the **parent** before the fork (`main.rs:75`).
If the home is idmapped by `move_mount`-ing an idmapped clone of `/home/robert`
over `/home/robert` *after* those binds exist, that clone is taken from the **real**
home and **shadows the files-slot binds** — the target would silently see the
*real* `~/.aws` instead of the brokered secret. This breaks exactly the common
wrapper shape (a files-slot projecting under `$HOME`).

**Required ordering: idmap `$HOME` *first*, then overlay the files-slot binds on
top of the idmapped home.** The decided placement (2630 spine) achieves this in
time: the parent idmaps `$HOME` **pre-fork**, and the child establishes the
files-slot binds **post-`setresuid`** — strictly later, so they overlay the
idmapped home. Do **not** `open_tree` the home clone with `AT_RECURSIVE` across a
subtree that already contains the X-owned files-slot staging (that would re-map the
staging through the 1700⇄X idmap and corrupt its ownership) — and since the binds
are layered *after* the home idmap, the home clone is taken before any staging
exists under it. Same caveat for any files-slot path under `$XDG_RUNTIME_DIR`. Add
a VM fixture that projects a files-slot secret to a path under `$HOME` for an
isolated wrapper and asserts the target reads the **brokered** content, not the
real home file.

**And the bind setup runs in the *child*, at its natural fsuid X (see 2630's
spine — DECIDED).** Once the home is idmapped it presents as **X-owned 0700**, and
the files-slot staging *source* tmpfs is **X-owned 0700**, so
`ensure_mount_target`'s mountpoint `create_dir` and the bind's source/target
traversal would **EACCES** for any *fsuid-1700* actor. The decided design
therefore does **not** do this in the parent pre-fork; it moves the staging tmpfs
create + materialize + files-slot mountpoints/binds into `run_child` **after**
`setresuid(X,X,X)`, where fsuid is already X (and `CAP_SYS_ADMIN` is retained
across the switch — `security/commoncap.c:1120-1146` — for the `mount`/`bind`
calls). **No `setfsuid` is used on this path**; the supervisor's only `setfsuid(X)`
window is the post-`waitpid` writeback walk. (The idmap of
`$HOME`/`$XDG_RUNTIME_DIR` itself stays in the parent pre-fork: `open_tree` must
traverse the real on-disk-1700 home as its owner, fsuid 1700; the child then
overlays the binds on top of the inherited idmapped home.)

### Module surface

- The set of idmapped paths is derivable (`$HOME` from the resolved target
  identity, `$XDG_RUNTIME_DIR` from `xdg_runtime_dir_base`/uid). Add an optional
  per-wrapper `idmapPaths` list for extra paths, default
  `[ "$HOME", "$XDG_RUNTIME_DIR" ]`. Keep it minimal.

## Acceptance

- New VM fixture `test-iso-home`: a script (run via an isolated wrapper) that
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
- **`NOSUID` enforced:** a fixture writes a `chmod u+s` binary into `$HOME` via
  the idmapped mount; assert (a) on disk it is owned by `testuser` with the setuid
  bit, and (b) exec'ing it *through the idmapped mount* does **not** confer the
  set-user-id (the process euid stays the caller's) — proving `MOUNT_ATTR_NOSUID`
  is in effect. Also assert no device node honoring (`NODEV`).
- The idmap `userns_fd` is **not** present in the target's `/proc/<pid>/fd`
  (closed before exec; cross-checked by 2645's fd-hygiene assertion).
- Graceful, explicit failure when `$HOME`'s filesystem lacks idmapped-mount
  support.
- `cargo test -p underwrap-ns`, `cargo fmt --check`, `nix build .#<vm test attr>`
  green.

## Out of scope

- `getuid()==caller` relabel (2650) — at this ticket the target still sees
  `getuid()==X` and files display as the dedicated user's name (access works;
  only the displayed identity differs). 2650 fixes the cosmetics.
- Supplementary groups; real session D-Bus; per-invocation uids.
