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
| `underwrap-admin-ui` | Operator web UI in front of the backend admin API | next to `underwrap-backendd` or on a jump host |
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

## End-to-end request flow

For a normal env-var wrapper:

```text
user runs work-jira issue list
  -> /run/wrappers/bin/work-jira                       (underwrap-exec, setgid)
  -> connect to /run/underwrap-work/wrapper-jira.sock
  -> underwrapd verifies the peer's effective GID matches that wrapper socket
  -> underwrapd authorizes and reads the secret from the central backend (HTTPS + mTLS)
  -> underwrap-exec drops privilege
  -> secret is handed to underwrap-ns over fd 3
  -> underwrap-ns creates the target environment
  -> target execs with the injected secret
```

When the backend policy puts the wrapper in **per-call approval** mode
the daemon adds one round-trip before the read: it issues a nonce,
waits for the wrapper's ssh-agent signature, verifies it against the
policy's `approval_key`, and only then fetches the secret.

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
in the backend's `PolicyDocument`:

- per-wrapper **approval mode** (`locked` / `unlocked{until?}` /
  `per-call-approval{approval_key}`)
- per-`(wrapper, resource, operation)` **rules** (permanent allow/deny)
- per-`(wrapper, resource, operation)` **grants** (time-bounded
  allow/deny)

See [The underwrap backend](backend.md) for the evaluation order,
policy data model, and API shape. `underwrapd` fetches the policy
document through the backend's authenticated service, evaluates it
locally for each request, and surfaces `NeedsApproval` to the wrapper
when per-call approval is required.

## Backend storage

`underwrap-backendd` persists everything under one of:

- **plain** — files owned by the backend daemon user,
- **encrypted** — ChaCha20-Poly1305 files keyed by a 32-byte master key
  file.

The service listener exposes only the narrow runtime surface
(`/v1/read`, `/v1/write`, `/v1/list`, `/v1/delete`, `/v1/status`). A
separate admin listener exposes the policy and audit surface.

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
      -> bind-mount into $HOME/<configured-path>
      -> target
```

For each directory binding, `underwrap-ns` materializes current backend
files into a namespace-private staging directory, bind-mounts it into
the target's home view (read-only when the binding is read-only), and
syncs changes back to the backend after exit for read-write bindings.

## Crate layout

```text
crates/
  underwrap-protocol        shared config, message families, IPC framing, fd passing
  underwrap-store           storage trait + backend implementations + mTLS HTTPS client
  underwrapd                frontend daemon on the target host
  underwrap-backendd        central authenticated HTTPS service
  underwrap-admin-ui        operator web UI in front of the admin API
  underwrap-exec            stage 1 wrapper (setgid)
  underwrap-ns              stage 2 wrapper (namespace setup)
  underwrap-secretsd-store  in-memory Secret Service data model
  underwrap-secretsd        Secret Service implementation for libsecret mode
  integration-tests         cross-component tests
```

### `underwrapd` internals

- `server.rs` — daemon startup and listener orchestration
- `startup.rs` — config validation, UID/GID resolution, socket binding
- `peer_auth.rs` — `SO_PEERCRED` and effective-GID checks
- `wrapper_flow.rs` — the wrapper request flow including the per-call
  approval round-trip
- `approval.rs` — pending-challenge store and SSH signature verification
- `policy.rs` — loads the backend's policy document and evaluates it
- `status.rs` — wrapper/credential status for the admin API audit
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
