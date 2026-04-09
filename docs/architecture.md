# Architecture

This document explains how underwrap is put together.

The short version:

- `underwrapd` is the main daemon on the target machine
- `underwrap-exec` is the setgid entry wrapper
- `underwrap-ns` is the namespace helper
- `underwrap-backendd` is the optional remote backend daemon
- `underwrap-secretsd` is the private Secret Service implementation used in libsecret mode

## The main components

| Component | Role | Typical location |
|---|---|---|
| `underwrapd` | Frontend daemon on the target host | NixOS machine running wrappers |
| `underwrap-exec` | Stage 1 wrapper: authenticate, fetch secret, drop privilege | `/run/wrappers/bin/<instance>-<wrapper>` |
| `underwrap-ns` | Stage 2 wrapper: create namespace, exec target | `/run/wrappers/bin/underwrap-ns` |
| `underwrapctl` | Administrative CLI for store/status/unlock/lock | target host |
| `underwrap-backendd` | Optional backend daemon for remote storage | trusted workstation or backend host |
| `underwrap-secretsd` | Secret Service provider for libsecret mode | inside the wrapper namespace |

## Why the wrapper chain has two stages

underwrap splits wrapper execution into two binaries:

1. **`underwrap-exec`**
   - installed setgid to a per-wrapper group
   - used for wrapper identity
   - talks to the daemon and receives secret material

2. **`underwrap-ns`**
   - installed with `cap_sys_admin` and `cap_setpcap`
   - used to create the PID namespace and related isolation
   - sets `PR_SET_NO_NEW_PRIVS` before exec of the final target and attempts to prune the capability bounding set where the kernel permits it

The split exists because NixOS `security.wrappers` does not let one wrapper binary carry both the setgid identity mechanism and the capability-based namespace setup cleanly.

## End-to-end request flow

For a normal env-var wrapper, the flow looks like this:

```text
user runs work-jira issue list
  -> /run/wrappers/bin/work-jira                         (underwrap-exec, setgid)
  -> connect to /run/underwrap-work/wrapper-jira.sock
  -> underwrapd verifies the peer's effective GID matches that wrapper socket
  -> underwrapd reads or derives the secret
  -> underwrap-exec drops privilege
  -> secret is handed to underwrap-ns over fd 3
  -> underwrap-ns creates the target environment
  -> target execs with the injected secret
```

## Identity model

The daemon does not trust a client-supplied wrapper name.

Instead:

- each wrapper binary is installed setgid to a dedicated group
- each wrapper gets its own wrapper socket
- the wrapper socket determines the expected wrapper
- the daemon obtains the peer process credentials from the kernel
- it reads the peer's effective GID from `/proc/<pid>/status`
- the peer effective GID must match the wrapper selected by that socket

This is one of the central design choices in underwrap.

## Storage model

underwrap is designed around a storage backend abstraction.

Available backend modes include:

- **plain** — local storage on the target machine
- **encrypted** — local storage with encryption at rest
- **remote** — a forwarded Unix socket to `underwrap-backendd`

From the wrapper's point of view, the flow is the same either way: it asks `underwrapd` for material, and `underwrapd` gets that material from its configured backend.

## Remote backend flow

In remote mode, the target machine still runs `underwrapd`, but secret storage moves elsewhere.

```text
wrapper -> underwrapd on target host -> SSH-forwarded Unix socket -> underwrap-backendd on workstation
```

This lets you keep secrets off the target machine entirely while preserving the same wrapper UX.

See [Remote backend](remote-backend.md).

## Libsecret flow

Libsecret mode adds two extra processes inside the wrapper namespace:

- `dbus-daemon`
- `underwrap-secretsd`

Conceptually:

```text
underwrap-exec
  -> underwrap-ns
      -> private dbus-daemon
      -> underwrap-secretsd
      -> target (for example gh)
```

The target sees a private `DBUS_SESSION_BUS_ADDRESS` and talks to `underwrap-secretsd` as its Secret Service provider.

`underwrap-secretsd` does not authenticate to the daemon on its own. Instead, the daemon creates a pre-authenticated channel during the original wrapper request and passes one end of that channel into the namespace.

## Directory credential flow

Directory credentials reuse the same basic capability pattern as libsecret mode.

Conceptually:

```text
underwrap-exec
  -> underwrap-ns
      -> namespace-private staging directory
      -> bind-mount into $HOME/<configured-path>
      -> target
```

For each configured directory credential binding:

1. the daemon authenticates the wrapper request
2. it creates a scoped channel for that credential root
3. `underwrap-exec` passes channel metadata to `underwrap-ns`
4. `underwrap-ns` materializes the current backend files in a namespace-private staging directory
5. it bind-mounts that directory into the target's home view
6. if the binding is read-only, it mounts it read-only
7. after target exit, `underwrap-ns` syncs file changes back to the backend for read-write bindings

Backend storage stays one key per file under `<ref>/files/<relative-path>`.

See:

- [GitHub via libsecret](gh-libsecret.md)
- [Security](security.md)

## Crate layout

```text
crates/
  underwrap-protocol        shared config, messages, IPC framing, fd passing
  underwrap-store           backend trait and implementations
  underwrapd                frontend daemon on the target host
  underwrap-backendd        backend daemon for remote mode
  underwrapctl              administrative CLI
  underwrap-exec            stage 1 wrapper (setgid)
  underwrap-ns              stage 2 wrapper (namespace setup)
  underwrap-secretsd-store  in-memory Secret Service data model
  underwrap-secretsd        Secret Service implementation for libsecret mode
  integration-tests         cross-component tests
```

## NixOS module responsibilities

The NixOS module is a large part of the architecture, not just packaging glue.

It generates and manages:

- instance users and groups
- per-wrapper groups
- setgid wrapper binaries
- the shared `underwrap-ns` wrapper
- daemon config files
- wrapper sidecar configs
- systemd services
- runtime and state directories
- tunnel users for remote mode

That is why underwrap is explicitly NixOS-first.

## Where to go next

- [Configuration reference](configuration.md)
- [Remote backend](remote-backend.md)
- [Security](security.md)
