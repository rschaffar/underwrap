# Task: Switch the wrapped target to its dedicated real uid in `underwrap-ns`

Parent: `2600_story_target_uid_isolation`
Depends on: `2620_task_dedicated_target_uid_and_capset`
Blocks: `2640`, `2645`, `2650`, `2660`

## Status: DONE for env mode (committed `2a2b57a`)

The uid switch — the protection this ticket exists to land — is implemented and
VM-green for **env-mode** isolated targets (`target_uid.rs`: root-owned-table
lookup keyed on the target path, `setresuid`/`setresgid`/`setgroups` to the
dedicated uid before `drop_caps`, post-drop `verify_switched`; cap_setuid chosen
over setuid-X per 2600). `local_target_uid_isolation` proves a same-UID caller is
denied the whole isolated subtree's `/proc` + secret.

The **files / libsecret** projection under `isolate` (the "Staging access — the
single `setfsuid` window" design + the `[2630]` files-slot SPINE steps below) was
**carved out to `2635_task_files_libsecret_under_isolate`** and is currently
fail-closed in `underwrap-ns::run()`. The design below stays here as the reference
2635 implements.

## Problem

This is the ticket that lands the protection. When the root-owned target→uid
table (2620) lists the target binary path, `underwrap-ns` must adopt that target's
dedicated **real** uid X (and gid Xgid) before exec, so that same-UID callers fail
the `__ptrace_may_access` uid gate and cannot read the target's
`environ`/`mem`/`maps`/`auxv`, ptrace it, or `process_vm_readv` it. Children
inherit X, so the whole subtree is covered.

## Authority — read the root-owned table, keyed on the target path

`underwrap-ns` reads `/etc/underwrap/target-uids.*` (root-owned; 2620) and looks
up **the absolute target binary path it is about to exec** (the daemon-chosen
path, already in argv):

- **path in the table →** isolated: take `{uid, gid}` from the table; validate
  (reject `0` / below the system-uid floor); `bail!` on a malformed table or an
  out-of-range uid (fail closed — never exec a target that should be isolated but
  isn't).
- **path not in the table →** unisolated: run the target as the caller (current
  behaviour, unchanged). The `[2630]`/`[2640]` steps below are skipped.

The uid is **never** sourced from `ExecBundle`, env, argv, or the daemon response
— only from the root-owned table. That removes the escalation/confused-deputy
vector structurally: the only uids reachable are the sandbox uids trusted root
config assigned. `underwrap-ns` runs as the caller's uid with `cap_setuid`/
`cap_setgid` file caps (2620), which is sufficient to switch — **no egid, no
privilege-drop relocation** (see 2600 "What we abandoned and why").

## Canonical ordering (the SPINE — 2640/2645/2650 insert their `[tagged]` steps)

The **complete** ordered sequence for the **isolated** path, across both
processes. Untagged steps already exist; `[2630]` are this ticket; the rest land
with their ticket. Caps (`cap_sys_admin,cap_setuid,cap_setgid`) are held from
start until the `drop_caps` points. **Ordering is security-critical — do not
reorder.**

```
PARENT, pre-fork  (holds caps; uid=caller):
  [2630] read the root-owned target-uids table; look up the target path.
         in table -> validate {uid X, gid Xgid} (reject 0 / sub-floor; bail on bad table).
         not in table -> unisolated; skip every [2630]/[2640] step below.
  unshare(CLONE_NEWPID | CLONE_NEWNS [| CLONE_NEWIPC | CLONE_NEWUTS])   # [2645] adds IPC/UTS
  make_mount_namespace_private()
  resource checkout                            # blob -> memory; INHERITED by the child via fork
  [2640] idmap $HOME / $XDG_RUNTIME_DIR  (open_tree REAL home + mount_setattr(IDMAP|NOSUID|NODEV) + move_mount)
         # MUST be here: open_tree traverses the real on-disk-caller 0700 home as its OWNER (fsuid=caller).
         # setattr/move is caps-only/fsuid-independent (detached fd). After move_mount /home presents X-owned,
         # so the files-slot binds onto it move to the child (fsuid naturally X).
  [2630] compute the deterministic files-slot staging LAYOUT (staging-path <-> mount-path) -
         PURE, no fs side effects. Kept by the parent for the writeback walk; the child mounts using it.
  fork()
  # NO staging tmpfs, NO materialize, NO bind mounts, NO setfsuid in the parent pre-fork.

CHILD = the target (run_child); ends up uid X:
  remount_proc()
  disable_core_dumps()                         # RLIMIT_CORE=0 (persists across setuid)
  setgroups([Xgid]); setresgid(X,X,X); setresuid(X,X,X)   # gid before uid; check returns, bail
         # caps RETAINED across this switch (no uid 0 either side - security/commoncap.c:1120-1146);
         # the X-facing mount/materialize work below relies on the retained CAP_SYS_ADMIN.
  [2630] create FILES-SLOT staging tmpfs uid=X,gid=Xgid,mode=0700  (if files)
         #   over a bare mountpoint dir (pre-created by the parent if /run/... isn't X-writable; mount is caps-only)
  [2630] materialize files-slot content from the INHERITED blob   (if files)   # fsuid X writes X-owned files
  [2640] files-slot mountpoints + plain bind mounts OVER the idmapped home   (if files)
         #   fsuid X traverses the X-presented idmapped home as OWNER and binds the X-owned staging - NO setfsuid.
         #   AFTER the idmap so they overlay it. PLAIN MS_BIND (staging is already uid=X; target sees X directly).
  [2630] [setup_mount_namespace() if libsecret]  # bus tmpfs uid=X + hide real bus; traverses the idmapped
         #   $XDG_RUNTIME_DIR as fsuid X (owner) - NO setfsuid. (was already in run_child.)
  [2650] unshare(CLONE_NEWUSER); deny setgroups; write uid_map/gid_map  # OPTIONAL; AFTER all mount work
  set_no_new_privs()
  [2645] keyctl(KEYCTL_JOIN_SESSION_KEYRING, NULL)
  [2645] (optional) seccomp filter - deny TIOCSTI/TIOCLINUX   (needs no_new_privs first)
  drop_caps()                                  # zero Eff/Prm/Inh/Amb (+bounding); clears ambient - AFTER all mounts
  [2630] VERIFY (in-process, userns-invariant): getresuid()/getresgid() all-equal (r==e==s),
         Cap{Eff,Prm,Inh,Amb}=0, setresuid(0,0,0) FAILS -> else bail.
         # NOT "== X": under [2650] /proc/self/* renders the mapped inside-view; the literal kuid==X / !=0
         # check is the VM test's job (root reads /proc/<pid>/status from OUTSIDE).
  make_process_non_dumpable()                  # belt-and-suspenders (exec resets it; uid gate is the real guard)
  [2645] setsid()                              # sever controlling tty (TIOCSTI guard)
  build_target_env()   (+ setup_libsecret here if libsecret - dbus-daemon/secretsd as X, using the bus tmpfs)
  close_channel_fd_for_exec()  +  [2645] close_range(3, ~0u, CLOEXEC)   # LAST fd touch before exec
  cmd.exec(target)

PARENT, post-fork  (the supervisor; stays uid=caller + caps until AFTER writeback):
  waitpid(child)
  [2630] setfsuid(X)/setfsgid(Xgid)        # THE SINGLE setfsuid window in the whole design
  [2630]   writeback: walk the X-owned staging via openat2(RESOLVE_NO_SYMLINKS|RESOLVE_BENEATH),
           O_NOFOLLOW|O_NONBLOCK, skip non-regular; read the X-owned libsecret-state.jcs
           # the window DOUBLES AS the symlink-TOCTOU guard: the walk has ONLY uid-X authority.
  [2630]   checkin over channel (fd op; fsuid-independent, harmless in-window)
  [2630] setfsuid(caller)/setfsgid(caller)   # CLOSE the window
  [2630] cleanup: rely on private-mount-ns teardown at parent exit (auto-unmounts the X-presented binds)
  [2630] drop_caps()   - for isolated targets drop HERE, not at the early main.rs:298 path
```

The original `run_child` maps to the untagged child lines; everything tagged is
added at that exact position.

## Implementation notes

- **Caps retained across the switch (the linchpin).** `1700 → X` involves no uid 0
  on either side, so the setuid cap-fixup never fires and `CAP_SYS_ADMIN`/
  `CAP_SETUID`/`CAP_SETGID` survive (`security/commoncap.c:1120-1146`, verified
  v7.1-rc6). This is what lets the child do all X-facing mount/materialize work at
  its natural fsuid X. The explicit `drop_caps` (below) is therefore load-bearing.
- Order: `setgroups([Xgid])` → `setresgid` → `setresuid`. Use `setresuid`/
  `setresgid` (not `setuid`), check returns, `bail!` on failure — **never exec a
  target that failed to drop to X** (it would run as the caller with the secret in
  a readable env). A retained `CAP_SETUID` would let the target `setresuid(0,0,0)`
  back, so `drop_caps` is mandatory.
- `set_no_new_privs` and `drop_caps` move to **after** the id switch (we need the
  caps to perform it). Confirm `caps.rs` still zeroes everything post-switch.
- **Verify the drop before exec (Setuid Demystified).** Assert the
  *userns-invariant* properties: `getresuid()`/`getresgid()` each return three
  equal values, `Cap{Eff,Prm,Inh,Amb}` empty, and a regain attempt
  `setresuid(0,0,0)` **fails**; `bail!` on any failure. Do **not** assert literal
  `Uid==X` in-process (under 2650 `/proc/self/*` renders the mapped inside-view);
  the host-side "real uid is exactly X, never 0/caller" check is the VM test's job
  (root reads `/proc/<pid>/status` from outside).
- **Dumpable note:** with real=eff=saved=X the target exec is a *normal* same-uid
  exec, so `begin_new_exec` resets dumpable to `SUID_DUMP_USER`. That's fine — the
  **uid gate** (caller ≠ X) is the protection, not dumpable.
  `make_process_non_dumpable()` before exec is defense-in-depth for the brief
  pre-exec window only; add a comment saying the exec resets it.

### Staging access — the single `setfsuid` window (DECIDED; verified v7.1-rc6)

The files-slot staging tmpfs and the libsecret bus tmpfs are *cross-uid* surfaces:
the uid-X target reads/writes them, and the caller-uid supervisor reads them once
at writeback. They can't be chowned (no `CAP_CHOWN`), and world-readable perms are
unsafe (a uid-X target replaces files with its umask, often `0600`, which a
caller-uid supervisor without `CAP_DAC_OVERRIDE` then can't read). Both tmpfs are
made **`uid=X,gid=Xgid,mode=0700`** and the access asymmetry is resolved by
**placing all X-facing work where fsuid is naturally X**, not by scattering
`setfsuid` calls:

- **Child side — zero `setfsuid` calls.** Everything touching an X-owned/X-presented
  path (staging create, materialize, files-slot binds over the idmapped home, the
  libsecret bus-hide that traverses the idmapped runtime dir) runs in `run_child`
  **after** `setresuid(X,X,X)`, where fsuid is already X and `CAP_SYS_ADMIN` is
  retained for the `mount`/`mount_setattr`/`bind` calls. The blob for materialize is
  already in the child (inherited via `fork`; decoded there for
  `build_target_env`/libsecret today). The files-slot bind is a **plain `MS_BIND`**
  of the uid=X staging (no idmap on the bind → target sees X directly). All mount
  work precedes the `[2650]` `CLONE_NEWUSER` unshare so caps still apply to the
  init-owned mount ns.
- **Parent side — exactly one `setfsuid(X)` window.** The only place the supervisor
  touches X-owned content is the post-`waitpid` writeback walk (staging tree +
  `libsecret-state.jcs`). Wrap *that* in `setfsuid(X)` … `setfsuid(caller)`
  (`setfsuid` is per-thread; keep this synchronous path off any threadpool). This
  single window is **a security feature, not a footgun**: it bounds the supervisor's
  authority to uid X while it walks a target-controlled tree (the symlink-TOCTOU
  defense, with `openat2(RESOLVE_*)` as the second layer). Mount cleanup needs **no**
  window — rely on private-mount-ns teardown at parent exit. **Requires keeping the
  supervisor's caps until after writeback:** today `run_parent` drops caps early at
  `main.rs:298` for the `!has_files` path — change it so an isolated target retains
  `CAP_SETUID`/`CAP_SETGID` through writeback and drops uniformly after.
- **Blob handoff / layout split (one structural change).** The parent computes the
  *deterministic* files-slot layout (staging-path ↔ mount-path) with **no fs side
  effects** and keeps it for the writeback walk; the child creates the tmpfs,
  materializes, and binds using the same scheme. Split `prepare_directory_mounts`
  (`directory_credential.rs`) accordingly. The bare staging mountpoint dir may need
  parent-side creation if `/run/...` isn't X-writable; the uid=X tmpfs is mounted
  over it by the child (mount is caps-only, not fsuid-gated).

Confidentiality rests on the **private mount namespace** (the caller cannot see
these mounts; concurrent invocations get their own) plus dbus peer-cred for the
bus. Add a VM assertion that the caller cannot see/enter these paths from the host
namespace.

### Symlink-TOCTOU on the writeback walk

At checkin the caller-uid supervisor walks target-controlled staging; a
target-planted symlink (e.g. `staging/x → ~/.ssh/authorized_keys`) could redirect
a supervisor read to a caller-owned path the target can't reach. Two defenses:
(a) the writeback walk runs inside the single `setfsuid(X)` window, bounding the
supervisor's authority *to X* during the traversal; and (b) resolve every walked
path with `openat2(RESOLVE_NO_SYMLINKS | RESOLVE_BENEATH)` rooted at the staging
dir (≥5.6). Do **not** rely on userspace `realpath` (racy). **Non-regular files:**
open walked entries `O_NOFOLLOW | O_NONBLOCK` and skip anything not a regular
file/dir, else a target-planted FIFO makes the supervisor's `open` block forever.

- `build_target_env` (`env.rs`): keep `HOME`/`XDG_RUNTIME_DIR` at the caller's real
  values (the idmapped mounts in 2640 make them accessible). Until 2640 lands, an
  isolated target that touches `$HOME` gets EACCES — gated behind `isolate`.

## Acceptance

- With `isolate = true`, the `test-iso-env` fixture runs as its dedicated uid X
  (assert via `/proc/<pid>/status` `Uid:` read **as root**, or a fixture printing
  `id -u`).
- **The protection (VM test):** as `testuser` (the caller), reading the isolated
  target's `/proc/<pid>/{environ,maps,auxv,mem}` and `ptrace(PTRACE_ATTACH)` all
  return **EPERM**. The unisolated `test-sleep` baseline still shows `environ`/
  `maps`/`auxv` **readable** and `mem`/ptrace denied — proving the difference is
  the uid switch. Flips the 2610 isolated branch from readable to denied.
- **Descendant coverage:** a fixture that `fork/exec`s a child `sleep`; assert the
  child's `/proc/<pid>/environ` is also EPERM for `testuser`.
- **Authority (hard):** `underwrap-ns` takes the uid only from the root-owned
  table keyed on the target path; `ExecBundle` carries no target id. It rejects a
  table-declared `0`/sub-floor and `bail!`s. A VM assertion confirms the isolated
  target's real uid is exactly the configured `underwrap-target-<n>` uid (never
  `0`, never the caller), and that a wrapper run with a deliberately bogus fd-3
  bundle cannot change which uid the target lands on.
- A failed `setresuid`/`setresgid` aborts the run (unit-test the error path).
- Parent supervisor still completes writeback/cleanup (stays at caller uid).
- **Files-slot and libsecret work under `isolate = true`** (neither needs `$HOME`,
  so both must function before 2640): the uid-X target reads its files-slot content
  and writes back; a libsecret store/lookup over the private bus succeeds with
  `dbus-daemon`/`secretsd` as X; the supervisor reads the staging /
  `libsecret-state.jcs` at writeback (inside the single `setfsuid(X)` window). Plus
  the VM assertion that the caller cannot see these paths from the host namespace.
- `cargo test -p underwrap-ns`, `cargo fmt --check`, `nix build .#<vm test attr>`
  green.

## Out of scope

- Home/socket access for the uid-X target (2640); the `getuid()==caller` relabel
  (2650). Until 2640, isolate only fixtures that don't need `$HOME` (`env`,
  `sleep`).
