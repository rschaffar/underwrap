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
6. `underwrap-exec` permanently drops its setgid identity, relocates every
   child-carried descriptor above the fixed slots, maps the resource channel to
   fd 20 and a private parent-liveness reader to fd 21, and sends the
   descriptor-free execution bundle over fd 3. Its waiting parent is the sole
   fd-21 pipe writer until `underwrap-ns` is reaped.
7. Because the capability-bearing `underwrap-ns` entry point is system-wide, it
   first resolves the selected instance's root-provisioned daemon account and
   requires fd 20 to be an `AF_UNIX` stream socket whose immutable
   `SO_PEERCRED` UID/GID matches that account. Only after this authority check it
   validates fd 21, arms parent-death `SIGKILL`, and checks for an already-dead
   parent. It retains that reader for one checked rearm immediately before the
   namespace fork, then closes it. It rejects before reading fd 3 or trusting the
   wrapper selector and target argv on any mismatch.
8. `underwrap-ns` resolves the dedicated target UID/GID from the root-owned
   `(instance, wrapper)` table. The UID/GID never comes from IPC or the daemon.
9. After the secret-free idmap helper has been reaped, `underwrap-ns` creates a
   private CLOEXEC authorization gate while fd 3 and fd 20 remain occupied. It
   reads fd 3, checks out and decodes the resource, and purely plans the complete
   files projection before namespace preparation, idmapped mounts, PID fork,
   staging, target-UID switching, or target-facing helpers.
10. The host supervisor unshares PID, mount, IPC, and UTS namespaces, makes
    mount propagation private, realizes idmapped caller-directory mounts, and
    forks a trusted namespace PID-1 relay. A second private sole-writer
    liveness pipe links the relay to the host. The relay also arms parent-death
    `SIGKILL` and rearms it immediately after the dedicated-UID switch because
    credential changes clear the setting. It keeps the reader through
    staging/setup, performs a final arm/check after capability/credential
    transitions, and actively polls it while waiting for PID 2; parent-death
    signals remain defense in depth.
11. The non-execing, zero-active-capability relay closes fd 20 and forks the
    configured target as namespace PID 2. PID 2 creates its own session, starts
    private helpers, owns the gate writer, and reports `Armed` immediately before
    exec; controlled returns report `Failed`. PID 1 reaps that direct child,
    writes one exact private target-status frame, and exits so the kernel
    terminates helpers and detached descendants.
12. The host observes the gate, cleanly reaps the relay, and only then parses one
    nonblocking status frame followed by EOF. Accepted gate, valid target status,
    and clean relay reap are all required for collection and one `UsageSignal`
    or `Checkin`. A valid controlled pre-exec status remains eligible for exact
    propagation when the gate suppresses resources.
13. `underwrapd` translates that request into the finalize API. The backend
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
- transfers the execution bundle through fd 3 and a private parent-liveness
  reader through fixed fd 21 while its waiting process owns the sole writer;
- executes and exactly reaps the capability-bearing namespace helper.

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
- after fd-20 authentication, validates fd 21 and race-closes the stage-1
  parent-death link before reading fd 3, trusting the wrapper selector or target
  argv, or performing namespace/mount/idmap work;
- creates and reaps the optional secret-free idmap helper before reading fd 3
  or Checkout bytes;
- creates a private CLOEXEC authorization gate, reads fd 3, checks out the
  resource, and completes semantic files planning before privileged projection
  effects;
- in the supervisor, unshares fresh PID, mount, IPC, and UTS namespaces, makes
  mount propagation private, and realizes idmapped caller-directory mounts
  before forking;
- keeps trusted code as namespace PID 1, arms a private host-parent death link,
  remounts `/proc`, switches to the resolved dedicated UID/GID, immediately
  rearms after that credential change, and realizes the selected projections;
- after projection setup, sets `PR_SET_NO_NEW_PRIVS`, clears and verifies the
  relay's active capability sets, closes fd 20, and forks the configured target
  as PID 2;
- closes PID 2's inherited relay-only lifecycle descriptors first, restores its
  target-visible `SIGCHLD` state, starts its separate session and private
  helpers, and reports `Armed` after final fd protection;
- has PID 1 poll the sole-writer host-liveness pipe while waiting for PID 2,
  fail closed on any event, reap PID 2, frame its exact status only after a final
  liveness check, and exit without exec, tearing down remaining namespace
  processes before host collection.

The host namespace supervisor observes the gate through EOF and then reaps PID
1. Finalization requires exact `Armed` followed by EOF without `Failed`, one
valid direct-target status frame followed by EOF, and a clean relay reap.
Libsecret additionally requires terminal publication EOF and an ACK-written
validated generation; a retained publication endpoint suppresses collection and
both terminal verbs. Malformed, truncated, retained-writer, early-closed,
failed-I/O, or failed-wait lifecycle states also suppress dispatch. The gate records an authorized lifecycle,
not proof that exec succeeded. A normal successful exec may finalize changes
made by the target. Only the false-positive case where the child dies after
`Armed` but before either CLOEXEC exec closure or complete `Failed` observation
is indistinguishable from closure; because no target ran, that narrow ambiguity
can at most finalize the fully materialized unchanged state. Supervisors without
files projection drop capabilities before waiting. Files supervisors retain
`CAP_SYS_ADMIN`, `CAP_SETUID`, and `CAP_SETGID` through target execution and the
post-run files-only fsuid read window, then drop them; the implementation does
not selectively drop `CAP_SYS_ADMIN` before the wait. After an authorized lifecycle,
finalization remains diagnostic-only and precedes exact propagation of target
exit codes and signals. Supervisor termination cascades inward with `SIGKILL`;
there is no graceful forwarding. Cancellation established before terminal
dispatch sends no terminal verb, but a request completed before later stage-1
death cannot be retracted without a backend transaction.

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
JCS-canonical JSON blob with an inclusive 2,097,152-byte complete-resource cap
and three slots:

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

Selected env entries are carried only in the authenticated fd-20 Checkout blob
and inserted into the final target environment; the small wrapper and fd-3
control messages carry only the env-projection selector. Runtime finalize rejects a submitted env section that differs from the resource
snapshot it loads, so the admin API/UI is the intended editor. The compare and
whole-blob save are not a transaction: concurrent writers are last-writer-wins,
and a stale runtime writeback can overwrite an admin edit that races between
them.

### Files projection

`underwrap-ns` materializes the files slot in a namespace-private `noswap`
tmpfs. Each configured `files.paths` entry becomes a bind mount rooted at the
caller's passwd home path. Paths must be clean, relative, unique, and
non-overlapping. Before projection effects, every clean files-slot key must
route to exactly one strict configured path, must not equal that mount path, and
must not be an ancestor or descendant of another regular-file key. Realization
consumes only that completed plan.

The target sees writable mounts. If the permission is read-only, changed bytes
are discarded and an audit event plus optional stderr warning records the
attempt. If the permission is writable, the complete canonical resource is
submitted for backend validation and replacement.

### Libsecret projection

Libsecret mode runs a private `dbus-daemon` and `underwrap-secretsd` beside the
target:

```text
host underwrap-ns
  ├── fd 20 to underwrapd
  └── sole publication receiver ◄══════════════════════════════╗
      namespace PID-1 relay                                    ║
        └── PID-2 setup process → configured target            ║
            ├── dbus-daemon                                    ║
            └── underwrap-secretsd ── sole publisher ═════════╝
                  └── one-shot initial section from PID-2 setup
```

The CLOEXEC stream socketpair is created before the PID namespace fork. The
host closes its publisher copy. PID 1 closes its copy immediately after forking
PID 2, PID 2 closes its copy immediately after forking secretsd, and dbus-daemon
explicitly closes it. Only the secretsd exec child temporarily clears CLOEXEC
for that exact endpoint; secretsd restores CLOEXEC immediately. The target and
other helpers retain neither endpoint.

`underwrap-secretsd` has no daemon socket. PID 2 sends the initial section
through a one-shot pipe. secretsd publishes generation 0 and waits for complete
host ACK before readiness; every later mutation publishes the next complete
canonical generation and waits for exact ACK or REJECT while retaining the
store write lock. The host validates canonical bytes, identifiers, aliases,
references, timestamps, the inclusive 1-MiB publication-body bound, and its
composition with immutable Checkout env/files under the complete-resource cap
before ACK.

After the unchanged blocking gate observation, the host drains publication while
polling relay `waitpid(WNOHANG)`. PID 1 independently polls liveness and reaps
PID 2. Following clean reap, terminal EOF permits the latest validated generation
whose ACK was completely written; final `EAGAIN` proves endpoint retention and
fails closed without fallback. ACK is invocation-local retention, not D-Bus
response delivery or backend durability. Resource Checkout/Checkin use a
separate bulk bound of the complete resource plus their message envelope; other
local control IPC remains limited to 64-KiB frames.

The private helpers start as PID 2 descendants and can be adopted by namespace
PID 1. PID 1 exit terminates every helper and descendant. The relay closes fd 20
before forking PID 2, so target, dbus-daemon, and secretsd never inherit that
resource channel. See [secretsd security](secretsd-security.md) for validation
and failure semantics.

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
the unlocked store. Key rotation stages and verifies a complete fresh
generation, activates it, and retains every displaced ciphertext generation in
a unique backend-host sibling workspace. Startup does not select or reconcile
those workspaces; trusted offline recovery and selective cleanup follow the
[backend procedure](backend.md#retained-rotation-generations).

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
- target-host Yama, kernel, ordinary-swap encryption, and zram swap-writeback
  assertions under the hardened posture.

The strongest posture admits no swap, ordinary swap admitted by the existing
random-key/LUKS assertion, or pure RAM-only zram. It rejects enabled typed
zram-generator swap sections with disk writeback because encryption cannot be
established from a backing-device path. The predicate permits writeback for
canonical mounted zram filesystem sections and ignores canonical non-zram
sections because neither is classified as memory swap. When zram-generator is
enabled, all final typed section and directive names must nevertheless be
canonical tokens and all string values, including those in mounted and non-zram
sections, must be single-line. The module evaluates declared NixOS configuration,
not arbitrary external `/etc` drop-ins or live runtime device state.

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
