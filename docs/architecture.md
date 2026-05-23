# Architecture

This document is the high-level map of underwrap. For the backend
surface in depth, see [The underwrap backend](backend.md).

## Components

| Component | Role | Typical location |
|---|---|---|
| `underwrapd` | Frontend daemon on the target host | NixOS machine running wrappers |
| `underwrap-exec` | Stage 1 wrapper: authenticate, fetch secret, drop privilege | `/run/wrappers/bin/<instance>-<wrapper>` |
| `underwrap-ns` | Stage 2 wrapper: create namespace, exec target | `/run/wrappers/bin/underwrap-ns` |
| `underwrap-backendd` | Central service: holds secrets + policy, authenticates `underwrapd` instances over mTLS | operator workstation or central server |
| `underwrap-admin-ui` | Operator web UI in front of the backend admin API (Next.js server/BFF) | next to `underwrap-backendd` or on a jump host |
| `underwrap-secretsd` | Secret Service provider for libsecret mode | inside the wrapper namespace |

The implementation deliberately splits process flow, authorization,
protocol handling, and storage into separate modules. Reviewers can read
the layers top-down without spelunking.

## Why the wrapper chain has two stages

`underwrap-exec` and `underwrap-ns` are installed as separate setgid
binaries:

1. **`underwrap-exec`**
   - installed setgid to a per-wrapper group
   - authenticates against `underwrapd` via kernel identity and
     receives secret material
2. **`underwrap-ns`**
   - installed with `cap_sys_admin` and `cap_setpcap`
   - creates the PID/mount namespace, sets `PR_SET_NO_NEW_PRIVS`, prunes
     the capability bounding set where the kernel permits it, and
     execs the final target

The split exists because NixOS `security.wrappers` cannot cleanly carry
both the setgid identity story and the capability-based namespace
story on one binary.

## Per-user PATH integration

`security.wrappers` is system-wide, so the canonical setgid binaries live
under instance-prefixed names like `/run/wrappers/bin/work-jira`. On top
of those, the NixOS module installs bare-named symlinks into each
authorized user's profile directory — `/etc/profiles/per-user/<user>/bin/jira`
points at `/run/wrappers/bin/work-jira` for any user listed in the
instance's `allowedUsers`. NixOS places per-user profile bins on each
user's `PATH`, so users invoke wrappers by their bare attribute name
without conflicting with other users on the same host. The bare-name
layer is a pure UX shim: kernel identity, setgid group, and sidecar
config all come from the prefixed binary the link resolves to.

## End-to-end request flow

For a normal env-var wrapper:

```text
user runs jira issue list
  -> /etc/profiles/per-user/<user>/bin/jira            (per-user symlink)
  -> /run/wrappers/bin/work-jira                       (underwrap-exec, setgid)
  -> connect to /run/underwrap-work/wrapper-jira.sock
  -> underwrapd verifies the peer's effective GID matches that wrapper socket
  -> underwrapd authorizes and reads the secret from the central backend (HTTPS + mTLS)
  -> underwrap-exec drops privilege
  -> secret is handed to underwrap-ns over fd 3
  -> underwrap-ns creates the target environment
  -> target execs with the injected secret
```

Per-invocation approval is intentionally out of scope. If you need
a human-in-the-loop prompt, layer it at the OS level (sudo, polkit)
in front of the wrapper.

## Local identity model

The daemon does not trust a client-supplied wrapper name.

Instead:

- each wrapper binary is installed setgid to a dedicated group,
- each wrapper gets its own wrapper socket,
- the socket determines the *expected* wrapper,
- the daemon reads the peer's effective GID from `/proc/<pid>/status`,
- the peer effective GID must match the wrapper selected by the socket.

This is one of the central design choices in underwrap. Nothing the
wrapper says on the socket identifies it — the kernel does.

## Central authorization model

Everything beyond "is this wrapper allowed to talk to me at all" lives
in the backend's `PolicyDocument`. The document is a flat list of
`WrapperPermission` rows keyed by `(client_instance, wrapper, resource_id)`.
Each row carries:

- a **mode**: `Locked`, `Unlocked`, or `UnlockedUntil { until_epoch }`,
- a per-row **`writable: bool`** flag deciding whether wrapper-side
  writebacks are accepted.

See [The underwrap backend](backend.md) for the evaluation order,
policy data model, and API shape. `underwrapd` does not evaluate
policy itself — it forwards every wrapper invocation to the backend,
which re-evaluates the document on every open and every finalize.

## Trust boundary

The daemon is intentionally a thin client of the backend. What it
**CAN** do:

- accept wrapper-socket connections, verify the peer's effective GID
  against the socket's expected wrapper, and refuse otherwise;
- map a verified wrapper invocation to a `(wrapper_name, resource_name)`
  pair and forward it to the backend over mTLS;
- relay an opaque resource blob from the backend to the wrapper
  (and back, on writeback) without inspecting it.

What the daemon **CANNOT** do:

- evaluate the policy document (it never has it);
- name a backend storage key on the wire (the runtime API speaks
  `(wrapper_name, resource_name)` only — `__underwrap/*` is
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

`underwrap-backendd` persists everything under one of:

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
the target's home view (read-only when the binding is read-only), and
syncs changes back to the backend after exit for read-write bindings.

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

## Crate layout

```text
crates/
  shared-protocol           shared config, message families, IPC framing, fd passing,
                            runtime wire types, policy + resource models
  shared-store              storage trait + plain/encrypted backend implementations
                            (backend-internal — nothing else implements it)
  shared-secretsd-store     in-memory Secret Service data model
  frontend-underwrapd       frontend daemon on the target host
  frontend-exec             stage 1 wrapper (setgid)
  frontend-ns               stage 2 wrapper (namespace setup)
  frontend-secretsd         Secret Service implementation for libsecret mode
  backend-backendd          central authenticated HTTPS service +
                            sole policy authority + embedded admin SPA
  integration-tests         cross-component tests
web/                        Vite + React + shadcn admin SPA (embedded into
                            backend-backendd via rust-embed)
```

### `underwrapd` internals

- `server.rs` — daemon startup, listener orchestration, lazy
  `BackendClient` open
- `backend_client.rs` — typed mTLS client for the three runtime
  endpoints; sync-callable from the wrapper-socket loop via a
  dedicated tokio worker thread
- `startup.rs` — config validation, UID/GID resolution, socket binding
- `peer_auth.rs` — `SO_PEERCRED` and effective-GID checks
- `wrapper_flow/` — the wrapper request flow (peer auth → open
  invocation on the backend → relay `ReadOk` + scoped channel)
- `wrapper_resource_channel.rs` — serves Checkout from the cached
  blob and turns Checkin / UsageSignal into finalize calls
- `status.rs` — wrapper/credential status for the admin API
- `wrapper_bindings.rs` — env/libsecret/directory binding selection

### `underwrap-ns` internals

- `main.rs` — high-level orchestration and fork/child/parent phasing
- `env.rs` — target environment assembly
- `caps.rs` — `no_new_privs` and capability drop logic
- `mounts.rs` — mount namespace setup and staging cleanup
- `libsecret.rs` — private D-Bus + `underwrap-secretsd` orchestration
- `directory_credential.rs` — directory materialization and sync

### `underwrap-secretsd` internals

- `dbus_service.rs` — shared types and path helpers
- `dbus_service/service.rs` — `org.freedesktop.Secret.Service`
- `dbus_service/collection.rs` — collection interface
- `dbus_service/item.rs` — item interface

## NixOS module responsibilities

The NixOS module generates and manages:

- instance users and groups
- per-wrapper groups
- setgid wrapper binaries
- the shared `underwrap-ns` wrapper
- daemon config files and sidecar configs
- systemd services
- runtime and state directories

This is why underwrap is NixOS-first: the module is load-bearing for
the identity model, not packaging sugar.

## Further reading

- [The underwrap backend](backend.md)
- [Deploying the central backend](remote-backend.md)
- [Admin UI](admin-ui.md)
- [Configuration reference](configuration.md)
- [Security](security.md)
