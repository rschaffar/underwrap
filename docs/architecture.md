# Architecture

underwrap separates persistent secret authority from local process execution.
The backend owns resources and policy; the NixOS frontend turns one authorized
resource into a short-lived target process.

## Components

| Component | Responsibility |
| --- | --- |
| `underwrap-backend` | HTTPS runtime API, admin API, embedded SPA, policy, resources, and persistent storage |
| `underwrapd` | Per-instance local broker; authenticates wrapper peers and relays the three runtime API calls |
| `underwrap-exec` | Setgid entry stage; requests an invocation, drops the instance-group identity, and hands execution state to `underwrap-ns` |
| `underwrap-ns` | Namespace supervisor; authenticates the fixed daemon channel, checks out the resource, switches target identity, projects slots, executes the target, and finalizes usage/writeback |
| `underwrap-secretsd` | Private Secret Service provider used inside a libsecret-enabled invocation |
| `underwrap-debug-tool` | Explicitly unsafe target selected only for an authorized debug variant |
| Admin SPA | Vite/React operator interface embedded in `underwrap-backend` |

The frontend wrapper chain is supported on NixOS. The backend is an ordinary
Linux HTTPS service.

## Identities and authorization units

Several identities serve different purposes:

- **Frontend instance:** one `underwrapd`, one Unix socket, one setgid entry
  point, one instance group, and one mTLS credential.
- **Runtime client instance:** the backend principal resolved from the presented
  client certificate fingerprint. It maps to exactly one store.
- **Wrapper/resource pair:** a configured wrapper name bound to one backend
  resource.
- **Permission row:** backend authorization keyed by
  `(runtime client instance, wrapper, resource)`.
- **Target UID:** one dedicated system UID/GID per `(frontend instance,
  wrapper)` pair.
- **Local Unix user:** the caller that starts the wrapper. This identity is not
  part of backend policy.

`allowedUsers` only installs per-user PATH shims. It does not restrict direct
execution of `/run/wrappers/bin/underwrap-<instance>`.

## Deployment shape

```text
operator browser
      │ HTTPS + session cookie + CSRF
      ▼
underwrap-backend ◄──────── HTTPS + mTLS ──────── underwrapd
      │                                                ▲
      │ resources + policy                             │ Unix socket
      │                                                │ effective-GID check
      ▼                                                │
StorageBackend                                   underwrap-exec
                                                       │ fd 3 + fd 20
                                                       ▼
                                                underwrap-ns
                                                       │
                                                       ▼
                                                    target
```

Runtime, admin API, and SPA routes share one configured HTTPS listener. TLS
accepts browser connections without a client certificate and runtime connections
with a presented client certificate; runtime handlers resolve and enforce the
mTLS principal.

## End-to-end invocation

For a normal wrapper invocation:

```text
caller runs <wrapper> [args]
  → per-user shim (when installed)
  → /run/wrappers/bin/underwrap-<instance> <wrapper> [args]
  → /run/underwrap-<instance>/wrapper.sock
  → underwrapd
  → POST /v1/wrapper-invocations
  → underwrap-exec
  → underwrap-ns
  → configured target
  → POST /v1/wrapper-invocations/finalize
```

The detailed sequence is:

1. The setgid instance entry point gives `underwrap-exec` the instance-group
   effective GID.
2. `underwrap-exec` connects to the instance's `0660` socket and sends the
   caller-selected wrapper name and invocation variant.
3. `underwrapd` reads the peer's effective GID, validates the wrapper name
   against root-owned config, resolves that wrapper's resource, and asks the
   backend to open the invocation.
4. The backend derives the runtime client instance from mTLS, resolves the
   wrapper/resource pair and permission row, evaluates the permission mode, and
   returns the canonical resource blob plus its current writable flag.
5. `underwrapd` chooses the target path: the configured target for `Real`, or
   `underwrap-debug-tool` for an authorized `Debug` invocation. It creates one
   scoped resource socketpair and sends the wrapper end with `SCM_RIGHTS`.
6. `underwrap-exec` permanently drops its setgid identity, relocates the
   received channel away from reserved descriptors, maps it to fixed fd 20, and
   sends the descriptor-free execution bundle to `underwrap-ns` over fd 3.
7. Because the capability-bearing `underwrap-ns` entry point is system-wide, it
   first resolves the selected instance's root-provisioned daemon account and
   requires fd 20 to be an `AF_UNIX` stream socket whose immutable
   `SO_PEERCRED` UID/GID matches that account. It rejects before reading fd 3 or
   trusting the wrapper selector and target argv on any mismatch.
8. `underwrap-ns` resolves the dedicated target UID/GID from the root-owned
   `(instance, wrapper)` table. The UID/GID never comes from IPC or the daemon.
9. `underwrap-ns` checks out the resource over authenticated fd 20, then creates
   PID, mount, IPC, and UTS namespaces, prepares projections, switches the
   target child to its dedicated UID, clears capabilities, and executes the
   daemon-chosen target.
10. After the target exits, the namespace supervisor compares the canonical
    post-run resource with the Checkout bytes. It sends either `UsageSignal` or
    `Checkin` over fd 20.
11. `underwrapd` translates that request into the finalize API. The backend
    re-evaluates policy and, for a writeback, re-checks writability and env
    immutability before persisting.

The runtime API is stateless: finalize does not rely on backend-side open-session
state.

## Local wrapper gate

Each frontend instance has:

- `/run/wrappers/bin/underwrap-<instance>` — system-wide setgid entry point;
- `/run/underwrap-<instance>/wrapper.sock` — `0660`, owned by the instance
  group;
- `underwrap-<instance>` — the daemon's primary group and the socket gate.

A non-root process reaches the daemon only by executing the setgid entry point.
The daemon confirms the resulting effective GID using `SO_PEERPIDFD` on the
default Linux 6.5+ hardened posture. The client-supplied wrapper name remains
application data: the daemon validates it and chooses the target itself.

This gate enforces traversal through the trusted wrapper chain. It does not
identify the local user and does not distinguish users for backend policy.

## Two wrapper stages

The entry flow is split because the setgid identity hop and namespace
capabilities have different privilege lifecycles.

### `underwrap-exec`

- starts through the per-instance setgid wrapper;
- authenticates to the instance socket;
- receives the daemon-chosen execution plan and resource channel;
- attempts to clear supplementary groups (`EPERM` is tolerated because a
  setgid-only process normally lacks `CAP_SETGID`) and permanently restores the
  caller's real, effective, and saved UID/GID;
- transfers the execution bundle through fd 3;
- executes the capability-bearing namespace helper.

### `underwrap-ns`

The NixOS capability wrapper supplies `CAP_SYS_ADMIN`, `CAP_SETUID`, and
`CAP_SETGID` for namespace, mount, idmap, and target-identity setup. The wrapper
configuration also names `CAP_SETPCAP`, but the NixOS security-wrapper mechanism
does not raise it into the ambient set.

`underwrap-ns`:

- makes itself non-dumpable before reading secret-bearing handoff state;
- authenticates fixed fd 20 from its socket domain/type and creation-time
  `SO_PEERCRED`, using the selected instance only to resolve the expected
  root-provisioned daemon UID/GID;
- rejects direct invocation before reading fd 3, trusting the wrapper selector
  or target argv, or performing namespace/mount/idmap work;
- creates fresh PID, mount, IPC, and UTS namespaces;
- makes mount propagation private and remounts `/proc` for the new PID
  namespace;
- resolves and switches to the dedicated target UID/GID;
- creates the selected projections;
- sets `PR_SET_NO_NEW_PRIVS`, clears capabilities, verifies that root cannot be
  regained, disables core dumps, starts a new session, and closes inherited
  descriptors;
- executes the target as PID 1 of the new PID namespace.

The namespace supervisor remains outside the target PID namespace long enough
to wait and finalize the resource. Env-only supervisors drop capabilities before
waiting. Files/libsecret supervisors retain `CAP_SYS_ADMIN`, `CAP_SETUID`, and
`CAP_SETGID` through target execution and the post-run fsuid read window, then
drop them; the implementation does not selectively drop `CAP_SYS_ADMIN` before
the wait.

## Target environment and host access

The target starts with a cleared environment. `underwrap-ns` adds:

- a fixed built-in passthrough set;
- additional variables named by `envPassthrough`;
- trusted fixed values from `envSet`;
- selected env-slot entries from the resource.

`HOME` defaults to the original caller's passwd home path, but the dedicated
target UID does not automatically gain access to that home. `accessPaths`
creates explicit idmapped bind mounts for caller-owned directories the target
needs. The idmap maps only the caller's UID/GID to the target UID/GID; other
owners remain unmapped.

`XDG_RUNTIME_DIR` points to a dedicated `0700` directory provisioned for the
target UID under `services.underwrap.targetRuntimeDirBase`. The caller's logind
runtime tree is not mounted automatically.

## Resource model

Each wrapper projects exactly one backend resource. A resource is one
JCS-canonical JSON blob with three slots:

```text
resource
  ├── env
  ├── files
  └── libsecret
```

The wrapper opts into any non-empty combination of these slots. The backend
keeps wrapper/resource pairs and permission rows separate so changing a
frontend wrapper's resource name cannot silently redirect an existing
permission to another secret.

### Environment projection

Selected env entries are inserted only into the final target environment.
Runtime finalize rejects a submitted env section that differs from the resource
snapshot it loads, so the admin API/UI is the intended editor. The compare and
whole-blob save are not a transaction: concurrent writers are last-writer-wins,
and a stale runtime writeback can overwrite an admin edit that races between
them.

### Files projection

`underwrap-ns` materializes the files slot in a namespace-private `noswap`
tmpfs. Each configured `files.paths` entry becomes a bind mount rooted at the
caller's passwd home path. Paths must be clean, relative, unique, and
non-overlapping; every files-slot key must route to one configured path.

The target sees writable mounts. If the permission is read-only, changed bytes
are discarded and an audit event plus optional stderr warning records the
attempt. If the permission is writable, the complete canonical resource is
submitted for backend validation and replacement.

### Libsecret projection

Libsecret mode runs a private `dbus-daemon` and `underwrap-secretsd` beside the
target:

```text
underwrap-ns (owns fd 20 to underwrapd)
  ├── dbus-daemon
  ├── underwrap-secretsd
  │     ├── one-shot initial section from underwrap-ns
  │     └── canonical state file on namespace-private tmpfs
  └── target → private DBUS_SESSION_BUS_ADDRESS
```

`underwrap-secretsd` has no daemon socket. `underwrap-ns` sends the initial
libsecret section through a one-shot pipe. secretsd keeps the keyring in memory
and atomically rewrites a tmpfs state file before acknowledging each D-Bus
mutation. When the target exits, the kernel terminates its PID-namespace
siblings; the supervisor reads the state file and performs final writeback over
fd 20.

The target, dbus-daemon, and secretsd all close fd 20 before exec. See
[secretsd security](secretsd-security.md) for the target-input boundary.

## Backend

The backend maps each active runtime client certificate fingerprint to one
runtime client instance and one store. It exposes exactly three runtime routes:

- `POST /v1/wrappers`
- `POST /v1/wrapper-invocations`
- `POST /v1/wrapper-invocations/finalize`

The wire carries wrapper and resource names, never a storage key. See
[Backend](backend.md) for the policy and data model.

Stores use either:

- `PlainBackend` — atomic plaintext files under the configured store directory;
- `EncryptedBackend` — ChaCha20-Poly1305 with an operator-held 32-byte key kept
  only in backend process memory.

Encrypted stores start locked after a backend restart. An operator supplies the
key through the admin UI/API; a canary verifies it before the backend installs
the unlocked store.

## NixOS module responsibilities

`nix/modules/underwrap.nix` provisions:

- per-instance daemon users/groups, state directories, runtime directories,
  sockets, and systemd services;
- one setgid entry point per instance;
- per-user convenience shims;
- one dedicated target user/group per `(instance, wrapper)`;
- the root-owned target-UID table;
- one runtime directory per target UID;
- the capability-bearing `underwrap-ns` wrapper;
- daemon and entry-stage configuration;
- target-host Yama and swap assertions under the hardened posture.

The module is part of the security architecture, not packaging convenience.

## Crate layout

```text
underwrap-protocol        config, IPC, runtime/admin DTOs, policy, resources
underwrap-backend-store   persistent storage implementations
underwrap-secretsd-store  in-memory Secret Service data model
underwrapd                local broker and runtime HTTPS client
underwrap-exec            setgid entry stage
underwrap-ns              namespace supervisor and target execution
underwrap-secretsd        private Secret Service provider
underwrap-debug-tool      unsafe debug target
underwrap-keygen          runtime client credential generator
underwrap-backend         runtime/admin HTTPS service and embedded SPA
integration-tests         real daemon/backend integration harness
web                       admin SPA
```

## Further reading

- [Security](security.md)
- [Backend](backend.md)
- [Configuration](configuration.md)
- [Deploying the backend](remote-backend.md)
- [Testing](testing.md)
