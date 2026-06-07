# Task: Support files / libsecret projection for an isolated target (staging-as-X + the single setfsuid writeback window)

Parent: `2600_story_target_uid_isolation`
Depends on: `2630_task_ns_setresuid_target_uid` (done), `2640_task_ns_idmapped_home_and_runtime` (done)
Blocks: `2660_task_comprehensive_isolation_vm_test`

## Status / why this exists

The env-mode per-target uid isolation epic is done and VM-green (2620/2630/2640/2645,
commits `2a2b57a` + `0834ce9`). This ticket is the **carved-out remainder of 2630**:
making the **files** and **libsecret** projection slots work when the wrapper sets
`isolate = true`.

Today that combination is **fail-closed**: `underwrap-ns::run()` bails with

> "isolate is not yet supported together with files or libsecret slots; refusing
> to run rather than fall back to an unisolated target"

— rather than silently running the target unisolated (which would put the secret
back in a same-uid-readable place). This ticket replaces that bail with a working
implementation. The design is already written up in **2630's "Staging access — the
single `setfsuid` window (DECIDED)"** section and the `[2630]` files-slot steps of
its canonical SPINE; this ticket implements exactly that.

## The problem (cross-uid sharing)

- **files mode**: the secret is materialized into a staging tmpfs and bind-mounted
  into the target's namespace at the configured home-relative paths. The uid-X
  target reads/writes that staging; the **caller-uid supervisor** (`underwrap-ns`
  parent) must read it back at end-of-run to write changes to the backend.
- **libsecret mode**: a private `dbus-daemon` + `underwrap-secretsd` serve the
  secret over a namespace-local bus; secretsd writes a `libsecret-state.jcs` the
  supervisor reads back.

The asymmetry: X-owned files (X's umask, often `0600`) aren't readable by the
caller-uid supervisor without `CAP_DAC_OVERRIDE`; caller-owned files aren't
writable by X; we hold no `CAP_CHOWN`. So neither "everything caller-owned" nor
"everything X-owned with world perms" is safe/correct.

## The design (from 2630 — implement it)

- **Staging tmpfs `uid=X,gid=Xgid,mode=0700`.** All X-facing work — create the
  staging tmpfs, materialize files-slot content from the inherited blob, bind it
  over the (idmapped) home, hide the real bus + launch dbus/secretsd — runs in the
  **child after `setresuid(X,X,X)`**, at natural fsuid X, with `CAP_SYS_ADMIN` still
  retained (the `1700→X` switch clears no caps; no uid 0 on either side). The
  files-slot bind is a plain `MS_BIND` of the uid=X staging (no idmap on the bind —
  the target sees X directly).
- **Exactly one `setfsuid(X)` window** in the supervisor: the post-`waitpid`
  writeback walk (staging tree + `libsecret-state.jcs`). It bounds the supervisor's
  authority to X over a target-controlled tree (the symlink-TOCTOU defense), with
  `openat2(RESOLVE_NO_SYMLINKS | RESOLVE_BENEATH)` + `O_NOFOLLOW|O_NONBLOCK`
  (skip non-regular files) as the second layer. Mount cleanup needs no window
  (private-mount-ns teardown at parent exit).
- **Keep the supervisor's caps until after writeback.** Today `run_parent` drops
  caps early for the `!has_files` path (`main.rs`); an isolated files/libsecret
  target must retain `CAP_SETUID`/`CAP_SETGID` through the writeback walk and drop
  uniformly after.
- **Split `prepare_directory_mounts`** (`directory_credential.rs`): the parent
  computes the deterministic staging↔mount layout (no fs side effects, kept for the
  writeback walk); the child creates the tmpfs + materializes + binds with the same
  scheme.
- libsecret: `dbus-daemon`/`secretsd` run as X using the bus tmpfs; the supervisor
  reads `libsecret-state.jcs` inside the single window.

## Acceptance

- Remove the fail-closed guard in `underwrap-ns::run()`; `isolate = true` works with
  `files` and with `libsecret`.
- VM: an isolated **files** wrapper — uid-X target reads its files-slot content and
  writes back; the supervisor persists the writeback (read inside the single
  `setfsuid(X)` window); read-only-grant writeback is dropped + warned as today.
- VM: an isolated **libsecret** wrapper — a store/lookup over the private bus
  succeeds with `dbus-daemon`/`secretsd` as X; state writeback persists.
- VM: the caller cannot see/enter the staging or bus paths from the host namespace
  (private mount ns), and the symlink-TOCTOU writeback walk is `openat2`-guarded.
- `cargo test -p underwrap-ns`, `cargo fmt --check`, `nix build .#<vm test attr>`
  green.

## Out of scope

- The optional `getuid()==caller` userns relabel (`2650`).
