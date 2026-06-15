# Architecture

This document is the high-level map of underwrap. For the backend
surface in depth, see [The underwrap backend](backend.md).

## Components

| Component | Role | Typical location |
|---|---|---|
| `underwrapd` | Frontend daemon on the target host | target machine running wrappers |
| `underwrap-exec` | Stage 1 wrapper: authenticate, fetch secret, drop privilege | `/run/wrappers/bin/underwrap-<instance>` |
| `underwrap-ns` | Stage 2 wrapper: create namespace, exec target | `/run/wrappers/bin/underwrap-ns` |
| `underwrap-backend` | Central service: holds secrets + policy, authenticates `underwrapd` instances over mTLS, serves the embedded admin SPA | operator workstation or central server |
| `underwrap-secretsd` | Secret Service provider for libsecret mode | inside the wrapper namespace |

The implementation deliberately splits process flow, authorization,
protocol handling, and storage into separate modules. Reviewers can read
the layers top-down without spelunking.

## Why the wrapper chain has two stages

The wrapper chain is two binaries:

1. **`underwrap-exec`** — installed once per instance as the setgid
   entry point `underwrap-<instance>`, setgid to the instance group
   `underwrap-<instance>`. It parses `[debug] <wrapper> [args]`,
   authenticates against `underwrapd` via the instance-group egid, and
   receives secret material plus the daemon-chosen target.
2. **`underwrap-ns`**
   - installed with `cap_sys_admin` (the module also lists `cap_setpcap`
     on the binary, but `nixpkgs` `security.wrappers` strips it from
     the ambient set at exec — see `docs/security.md`)
   - creates the PID/mount namespace, sets `PR_SET_NO_NEW_PRIVS`, clears
     the permitted/effective/inheritable cap sets via `capset`, attempts
     bounding-set pruning best-effort, and execs the final target

The split exists because the setgid identity story and the
capability-based namespace story do not cleanly coexist on a single
binary.

## Per-user PATH integration

`security.wrappers` is system-wide, so the one setgid entry point per
instance lives under `/run/wrappers/bin/underwrap-<instance>` (e.g.
`underwrap-work`). On top of it, the NixOS module installs per-user convenience
shims into each authorized user's profile directory —
`/etc/profiles/per-user/<user>/bin/jira` and `.../bin/debug-jira` for any
user in the instance's `allowedUsers`. Those shims are plain,
unprivileged scripts that just `exec /run/wrappers/bin/underwrap-<instance>
[debug] jira "$@"`; they carry no setgid bit and no config. NixOS places
per-user profile bins on each user's `PATH`, so users invoke wrappers by
their bare attribute name. All kernel identity and target selection
happen in `underwrap-<instance>` and the daemon, never in the shim.

## End-to-end request flow

For a normal env-var wrapper:

```text
user runs jira issue list
  -> /etc/profiles/per-user/<user>/bin/jira            (per-user shim script)
  -> /run/wrappers/bin/underwrap-work jira issue list          (underwrap-exec, setgid)
  -> connect to /run/underwrap-work/wrapper.sock        (0660, instance group)
  -> underwrapd checks the peer's effective GID == the instance group
     (race-free via SO_PEERPIDFD), then validates the wrapper name "jira"
     against config and relays { wrapper_name, resource, variant } to the
     backend
  -> backend re-evaluates the policy for the variant and returns the blob
  -> underwrapd returns the daemon-CHOSEN target + env + secret
  -> underwrap-exec drops privilege
  -> secret + target are handed to underwrap-ns over fd 3
  -> underwrap-ns creates the target environment
  -> the daemon-chosen target execs with the injected secret
```

Per-invocation approval is intentionally out of scope. If you need
a human-in-the-loop prompt, layer it at the OS level (sudo, polkit)
in front of the wrapper.

## Local identity model

Identity splits into a kernel gate and a daemon gate:

- **Kernel gate.** One `0660` socket per instance,
  `/run/underwrap-<instance>/wrapper.sock`, owned by the instance group
  `underwrap-<instance>`. A process can only open it if it holds that
  group as effective GID, and the only way to get it is to exec the
  setgid `underwrap-<instance>` binary. The daemon re-confirms the peer egid
  (race-free via `SO_PEERPIDFD`, `SO_PEERCRED` + `/proc` fallback) before
  reading the request.
- **Daemon gate.** The wrapper name and variant in the request are
  client-supplied. The daemon treats the name as a *selector*: it
  rejects unknown names and the reserved `debug` token, and then —
  crucially — **chooses the target binary itself** (configured target
  for `Real`, debug tool for `Debug`), only after the backend authorizes
  the variant. A forged `variant = Real` cannot reach the debug tool,
  and `argv[0]` is never consulted.

## Central authorization model

Everything beyond "is this wrapper allowed to talk to me at all" lives
in the backend's `PolicyDocument`. The document is a flat list of
`WrapperPermission` rows keyed by `(client_instance, wrapper, resource_id)`.
Each row carries:

- a **mode**: `Locked`, `Unlocked`, `UnlockedUntil { until_epoch }`, or
  `UnsafeDebugUntil { until_epoch }` (the only mode that admits the
  `Debug` variant),
- a per-row **`writable: bool`** flag deciding whether wrapper-side
  writebacks are accepted.

See [The underwrap backend](backend.md) for the evaluation order,
policy data model, and API shape. `underwrapd` does not evaluate
policy itself — it forwards every wrapper invocation to the backend,
which re-evaluates the document on every open and every finalize.

## Trust boundary

The daemon is intentionally a thin client of the backend. What it
**CAN** do:

- accept connections on the instance socket, verify the peer's
  effective GID == the instance group, validate the client-supplied
  wrapper name against config, and refuse otherwise;
- map a verified wrapper invocation to a `(wrapper_name, resource_name)`
  pair and forward it to the backend over mTLS;
- relay an opaque resource blob from the backend to the wrapper
  (and back, on writeback) without inspecting it.

What the daemon **CANNOT** do:

- evaluate the policy document (it never has it);
- name a backend storage key on the wire (the runtime API speaks
  `(wrapper_name, resource_name)` only — `control/*` is
  unreachable);
- decide whether a writeback is allowed (the backend re-checks the
  `writable` flag on every finalize);
- mutate the `env` section of a resource blob on writeback (the
  backend rejects any change to `env` with
  `InvalidBlob { reason: "env-section-immutable" }`).

A buggy or malicious daemon can only reach what its mTLS client
certificate's principal already has explicit `WrapperPermission`
rows for.

## Backend storage

`underwrap-backend` persists everything under one of:

- **plain** — files owned by the backend daemon user,
- **encrypted** — ChaCha20-Poly1305 files keyed by a 32-byte master key
  file.

The runtime listener exposes exactly three mTLS endpoints to
`underwrapd`:

- `POST /v1/wrappers` — register the daemon's `(wrapper_name,
  resource_name)` pairs at startup.
- `POST /v1/wrapper-invocations` — open one invocation. Returns
  `Allow { resource_id, blob_b64, writable }` or `Deny { reason }`.
- `POST /v1/wrapper-invocations/finalize` — close one invocation,
  optionally with a writeback blob. Re-checks writability and
  env-immutability server-side.

A separate admin listener exposes the policy and admin surface.

## Libsecret flow

Libsecret mode adds two extra processes inside the wrapper namespace:

- `dbus-daemon` (private to the namespace)
- `underwrap-secretsd`

```text
underwrap-exec
  -> underwrap-ns
      -> private dbus-daemon
      -> underwrap-secretsd  (talks back via the daemon's scoped channel fd)
      -> target (for example gh)
```

The target sees a private `DBUS_SESSION_BUS_ADDRESS` and uses
`underwrap-secretsd` as its Secret Service provider. `underwrap-secretsd`
does not authenticate to the daemon on its own; the daemon creates a
pre-authenticated channel during the original wrapper request and
passes one end into the namespace.

## Directory credential flow

Directory credentials reuse the same channel pattern as libsecret mode:

```text
underwrap-exec
  -> underwrap-ns
      -> namespace-private staging directory
      -> bind-mount into $HOME/<configured-path>...
      -> target
```

For each directory binding, `underwrap-ns` materializes current backend
files into a namespace-private staging directory, bind-mounts it into
the target's home view (always read-write to the target — the backend,
not a kernel mount flag, is the sole write authority), and syncs changes
back to the backend after exit. When the wrapper permission is read-only
the end-of-run writeback is rejected server-side and the change is
dropped (with one journal event + stderr line per affected resource).

The same backend resource can be projected at multiple paths within one
wrapper by configuring `files.paths` as a list. Each configured path
gets its own staging directory, and every file key in the resource
blob is routed to the staging dir whose mount path is a prefix of the
key (e.g. `.config/gh/hosts.yml` lands under the `.config/gh` mount).
A blob key that does not match any configured mount path aborts the
wrapper — the backend stores keys opaquely, but the frontend will not
silently drop content that has nowhere to go. Mount paths in
`files.paths` are required to be non-overlapping so routing is
unambiguous; the NixOS module rejects configurations that violate
this.

## Runtime directory for isolated targets

Every wrapped target runs as a dedicated `underwrap-target-*` uid X (see the
target-UID isolation work). A runtime directory (`XDG_RUNTIME_DIR`) is a
**per-uid** resource — the kernel/desktop convention is one `0700` dir owned by
the uid using it — so the honest reflection of running as X is that **X gets
X's own runtime directory**, not the caller's. This is not sandboxing; it is
just giving the executing uid the runtime dir that uid is entitled to.

That directory has to be **provisioned by the system**. logind only creates
`/run/user/<uid>` for *login sessions*, and X is a `nologin` system uid that
never logs in, so it never gets one. `underwrap-ns` cannot create one at runtime
either: it holds `cap_sys_admin,cap_setpcap,cap_setuid,cap_setgid` but **not**
`CAP_DAC_OVERRIDE`/`CAP_CHOWN`, and `/run/user` is root-owned (logind's tree). So
the NixOS module provisions, via `systemd-tmpfiles`, one `0700` self-owned dir
per target user under a dedicated base (`services.underwrap.targetRuntimeDirBase`,
default `/run/underwrap-runtime` — deliberately **not** under `/run/user`). The
directory is keyed on the target user *name* (known at module-eval time);
`underwrap-ns` derives the identical path from X via `getpwuid(X).pw_name`, so
the two always agree without pinning a numeric uid. `XDG_RUNTIME_DIR` is then
`<base>/<target-user-name>`, forced into the target environment (an `envSet`
override is rejected).

The caller's *own* session runtime dir is intentionally **not** wired into the
namespace. If a tool needs specific caller-owned directories (config/state),
list them in `accessPaths`: each is exposed through an idmapped bind mount that
maps exactly the caller's uid/gid to X's, so the target reads and writes those
files as the caller while every other owner stays unmapped. The idmap is applied
**best-effort per submount**: the directory is cloned recursively (so nested
mounts come along) and each mount is idmapped individually, so an idmappable
submount (a nested ZFS dataset under `~`, an ext4/xfs/btrfs bind) is mapped while
a non-idmappable one (fuse — desktop portals, sshfs, AppImage) is skipped with a
warning instead of failing the whole mount with `EINVAL`. The named directory's
own filesystem must be idmappable, or the access path is a hard error.

This all sits on the right side of the threat model (see `CLAUDE.md`): none of it
exposes the brokered secret to the caller. The target is handed a working
runtime dir, so it is not made non-functional; only the caller's live *session*
(its `/run/user` sockets — wayland, ssh-agent, the session D-Bus) is out of
scope, by choice.

### Rejected alternatives

- **Idmap the caller's `/run/user/<uid>` into the namespace (the first attempt,
  default-on).** A category mismatch: `/run/user` is a live session tree of
  sockets and foreign fuse filesystems, not caller-owned config/state. It broke
  in practice — a recursive `mount_setattr(IDMAP)` returns `EINVAL` on the fuse
  portals (`xdg-document-portal`, `gvfsd-fuse`) a desktop session mounts there.
  It also leaks target scratch into the caller's session, depends on the caller
  having a session at all, and on a `libsecret` wrapper re-exposes the caller's
  real session bus at `$XDG_RUNTIME_DIR/bus`. Even reduced to a default-off
  opt-in it was removed entirely — not worth the surface until a concrete need
  appears.
- **A private per-invocation tmpfs.** A namespace-local hack for "X has no
  runtime dir": ephemeral, and non-canonical — a provisioned per-uid directory
  is what a runtime dir actually *is*.
- **A shared `1777` base keyed on the caller uid.** World-writable, cross-uid
  collisions, wrong ownership.

## Crate layout

```text
crates/
  underwrap-protocol       shared config, message families, IPC framing, fd passing,
                           runtime wire types, policy + resource models
  underwrap-backend-store  storage trait + plain/encrypted backend implementations
                           (backend-internal — nothing else implements it)
  underwrap-secretsd-store in-memory Secret Service data model
  underwrapd               frontend daemon on the target host
  underwrap-exec           stage 1 wrapper (setgid)
  underwrap-ns             stage 2 wrapper (namespace setup)
  underwrap-secretsd       Secret Service implementation for libsecret mode
  underwrap-keygen         underwrap-keygen: client-credential / key generation
  underwrap-debug-tool     underwrap-debug-tool: the target the daemon execs
                           for variant = Debug (summary / shell / exec, with
                           read-write access to the projected secret)
  underwrap-backend        central authenticated HTTPS service +
                           sole policy authority + embedded admin SPA
  integration-tests        cross-component tests
web/                       Vite + React + shadcn admin SPA (embedded into
                           underwrap-backend via rust-embed)
```

### `underwrapd` internals

- `server.rs` — daemon startup, listener orchestration, lazy
  `BackendClient` open
- `backend_client.rs` — typed mTLS client for the three runtime
  endpoints; sync-callable from the wrapper-socket loop via a
  dedicated tokio worker thread
- `startup.rs` — config validation, UID/GID resolution, socket binding
- `peer_auth.rs` — peer credential resolution (race-free via
  `SO_PEERPIDFD` where available, with a `SO_PEERCRED` + `/proc`
  fallback) and the effective-GID check against the instance group
- `wrapper_flow/` — the wrapper request flow (peer auth → open
  invocation on the backend → relay `ReadOk` + scoped channel)
- `wrapper_resource_channel.rs` — serves Checkout from the cached
  blob and turns Checkin / UsageSignal into finalize calls
- `audit.rs` — structured audit-event emission (secret IDs, never values)
- `wrapper_bindings.rs` — env/libsecret/directory binding selection

### `underwrap-ns` internals

- `main.rs` — high-level orchestration and fork/child/parent phasing
- `env.rs` — target environment assembly
- `caps.rs` — `no_new_privs` and capability drop logic
- `mounts.rs` — mount namespace setup and staging cleanup
- `libsecret.rs` — private D-Bus + `underwrap-secretsd` orchestration
- `directory_credential.rs` — directory materialization and sync
- `target_uid.rs` — dedicated-uid resolution (root-owned table), switch, and the
  target's own runtime-dir path derivation
- `idmap.rs` — `accessPaths` idmapped bind mounts (best-effort per submount)

### `underwrap-secretsd` internals

- `dbus_service.rs` — shared types and path helpers
- `dbus_service/service.rs` — `org.freedesktop.Secret.Service`
- `dbus_service/collection.rs` — collection interface
- `dbus_service/item.rs` — item interface

## NixOS module responsibilities

The NixOS module generates and manages:

- instance users and the single instance group `underwrap-<instance>`
- the one setgid entry point per instance, `underwrap-<instance>`
- per-user convenience shims (`<wrapper>` / `debug-<wrapper>`)
- the shared `underwrap-ns` wrapper
- the daemon config (`/etc/underwrap/daemon/<instance>.yaml`) and the
  minimal underwrap-exec sidecar (`/etc/underwrap/instances/<instance>.yaml`)
- systemd services
- runtime and state directories

The NixOS module is load-bearing for the identity model, not just
packaging sugar — it provisions the privileged pieces (setgid binary,
file capabilities, declarative user/group, systemd units) the wrapper
chain depends on.

## Further reading

- [The underwrap backend](backend.md)
- [Deploying the central backend](remote-backend.md)
- [Admin UI](admin-ui.md)
- [Configuration reference](configuration.md)
- [Security](security.md)
