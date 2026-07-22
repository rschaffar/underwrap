# Security

This document defines underwrap's threat model, hardened-posture claim,
security invariants, mechanisms, and limitations.

> [!WARNING]
> underwrap is pre-release security software. The design is tested and intended
> to fail closed, but it has not been formally verified or independently audited
> end to end.

## Contents

- [Security claim](#security-claim)
- [Threat model](#threat-model)
- [Design invariants](#design-invariants)
- [Local wrapper boundary](#local-wrapper-boundary)
- [Target isolation](#target-isolation)
- [Environment and process state](#environment-and-process-state)
- [Resource projections](#resource-projections)
- [Backend security](#backend-security)
- [Permission model](#permission-model)
- [Audit and diagnostics](#audit-and-diagnostics)
- [Limitations and operational assumptions](#limitations-and-operational-assumptions)

## Security claim

### Preconditions

The strongest claim applies only when all of these conditions hold:

1. The frontend runs on NixOS with
   `services.underwrap.hardenTargetHost = true`.
2. The kernel is Linux 6.5 or newer, so wrapper peer authentication uses
   `SO_PEERPIDFD` rather than the weaker PID-based fallback.
3. `kernel.yama.ptrace_scope = 2`. The module sets this with `mkDefault`; an
   operator override can lower it and therefore weakens ATTACH-mode protection.
4. Every configured swap device is encrypted, or the host has no swap. The
   module asserts this under the hardened posture.
5. The NixOS module's root-owned wrappers, config, target-UID table, users,
   groups, capabilities, and systemd units have not been modified by an
   attacker.
6. The backend server certificate is trusted by the daemon, and the runtime
   client key remains readable only by the per-instance daemon user.
7. The configured target is a narrow helper that does not print, log, persist,
   or transmit the secret and does not accept caller-controlled behavior that
   causes such disclosure.

The final condition is load-bearing. underwrap gives the configured target the
projection. Kernel isolation cannot stop that target from disclosing what it is
trusted to use.

### Claim

Under those preconditions, there is no known kernel-mediated path for a
non-root local caller to read the raw brokered secret from the wrapper or target
process tree. Every wrapped target and its descendants run under a dedicated
UID for the `(instance, wrapper)` pair, distinct from the caller and from other
wrappers.

This is a scoped engineering claim, not a proof. Implementation defects in
underwrap, dependencies, or the kernel remain possible.

### Operational cost

Yama `ptrace_scope=2` blocks unprivileged `strace`, `gdb`,
`process_vm_readv`, and ptrace attachment against ordinary user processes. An
operator can override the module's `mkDefault`, but doing so weakens the
ATTACH-mode part of the claim. The dedicated target UID continues to protect
READ-mode `/proc` entries such as `environ`; Yama does not govern those reads.

## Threat model

### Protected assets

underwrap protects:

- resource values in env, files, and libsecret slots;
- the runtime client's mTLS private key;
- encrypted-store master keys while held by the backend;
- private projection mounts, the private D-Bus endpoint, and authenticated
  resource-channel descriptors.

Resource names, wrapper names, process metadata, mountpoint names, and policy
deny reasons are not secrets.

### Untrusted caller

The caller and other non-root processes owned by that caller are untrusted with
respect to the brokered secret. Assume they can:

- execute any system-wide instance entry point;
- choose any configured wrapper name and target arguments;
- control their original environment and inherited stdio;
- inspect host-visible process metadata and files allowed by normal DAC;
- race wrapper invocations and send malformed local IPC after reaching the
  setgid entry path.

`allowedUsers` only installs convenient PATH shims. It is not an access-control
list. Backend policy is keyed by runtime client instance, wrapper, and resource;
it does not distinguish local Unix users.

The setgid/Unix-socket boundary therefore guarantees **execution through the
trusted entry chain**, not per-user authorization. Put sudo, polkit, or another
OS-level gate in front when local-user policy or a prompt is required.

### Trusted target, untrusted target input

The configured target is trusted with the projection it receives. underwrap
does not prevent it from leaking that projection through output, files, network
requests, plugins, helpers, or application behavior.

Protocol input from the target is still treated as untrusted for robustness and
backend integrity. In particular, `underwrap-secretsd` validates and bounds
D-Bus mutations even though the target is trusted to use the contained secret.
See [secretsd security](secretsd-security.md).

### Trusted computing base

The security model trusts:

- the Linux kernel and NixOS wrapper implementation;
- host root and root-owned configuration;
- `underwrap-backend` and its host root;
- the configured storage backend;
- TLS, cryptographic, and serialization dependencies.

### Non-goals

underwrap does not protect against:

- root or kernel compromise;
- a malicious or compromised backend;
- a target that reveals its own projection;
- broad wrapper behavior or overpowered downstream credentials;
- denial of service by callers or targets;
- general containment of the target as an adversarial workload;
- per-invocation human approval;
- local-user authorization through `allowedUsers`;
- a persistent, tamper-evident audit ledger.

## Design invariants

### 1. The backend is the only persistent secret store

All durable secret material is written through `underwrap-backend`'s
`StorageBackend`. The frontend daemon caches one opaque canonical resource blob
in memory for the lifetime of a resource channel.

The documented ephemeral exception is namespace-private tmpfs used to stage
files and libsecret state during one invocation. The frontend must not add:

- ordinary temporary files containing secrets;
- JSON sidecars or local databases;
- disk caches;
- alternate persistence paths that bypass backend policy.

### 2. The kernel gates entry; the daemon validates selection

The instance socket is `0660` and group-owned by
`underwrap-<instance>`. A non-root process must execute the root-installed
setgid instance entry point to acquire that effective GID and connect.

The wrapper name remains caller-supplied data. `underwrapd` rejects unknown
names and the reserved `debug` token, resolves the configured resource, and
chooses the target after backend authorization. Neither `argv[0]` nor a
caller-supplied target path participates in authorization.

### 3. Target identity comes only from root-owned state

`underwrap-ns` resolves the target UID/GID from
`/etc/underwrap/target-uids`, keyed by `(instance, wrapper)`. The table is
root-owned and generated by the NixOS module.

The target UID/GID must never appear in:

- the backend runtime API;
- the daemon's wrapper response;
- `ExecBundle`;
- argv;
- a caller-controlled environment variable.

A missing selector, table, row, user, or group is fatal. There is no fallback to
the caller's UID or root.

### 4. Secret-bearing channels are authenticated before use

The local resource socketpair is created by `underwrapd` only after wrapper-peer
authentication, wrapper validation, and backend authorization. Linux records the
creator's credentials on an `AF_UNIX` socketpair and preserves them when an
endpoint crosses `SCM_RIGHTS`, `dup2`, fork, and exec.

`underwrap-ns` is a system-wide capability wrapper. Its first trust decision is
therefore to require fixed fd 20 to be an `AF_UNIX` `SOCK_STREAM` endpoint whose
`SO_PEERCRED` UID/GID matches the root-provisioned
`underwrap-<instance>` daemon account. A mismatch, malformed descriptor, missing
account, root identity, or daemon/target identity overlap aborts before fd 3 is
read, the wrapper selector or target argv is trusted, or privileged namespace
setup begins.

The fd number is not carried in `ExecBundle`; fd 20 is part of the fixed local
ABI. No bearer token in argv or environment re-authenticates later stages. A
process holding a legitimate fd 20 already holds the scoped resource capability,
so descriptor confidentiality remains load-bearing.

### 5. Descriptors do not cross into the target

- fd 3 carries descriptor-free execution metadata from `underwrap-exec` to
  `underwrap-ns` and is read only after fd-20 peer authentication;
- fixed fd 20 is the scoped resource channel between `underwrap-ns` and
  `underwrapd`; its number cannot be redirected through bundle data;
- dbus-daemon and secretsd close fd 20;
- the final target closes or receives close-on-exec for every fd above stderr.

Leaking fd 20 would give the target a direct authenticated resource capability
and is a security defect.

### 6. Active security posture fails closed

Handoff peer authentication, namespace creation, mount setup, target-UID
switching, capability drop, credential checks, and descriptor cleanup abort on
failure.

After the secret-free idmap helper is reaped, `underwrap-ns` creates a private
CLOEXEC child-to-parent authorization gate while fixed fd 3 and fd 20 remain
occupied. The supervisor then unshares PID, mount, IPC, and UTS namespaces,
makes mount propagation private, realizes idmapped caller-directory mounts, and
forks. The child enters the new PID namespace, remounts `/proc`, and performs
the remaining controlled setup and projection materialization. It reports
`Armed` only after resource-channel closure and final descriptor protection; a
controlled return reports `Failed`. The supervisor observes the gate through
EOF and then reaps the child. It collects staging and sends an end-of-run
resource verb only when the exact `Armed` marker followed by EOF, with no
failure or extra bytes, is paired with a successful reap. Early EOF, an observed
failure, malformed or truncated input, gate I/O failure, or wait failure
suppresses both collection and `Checkin`/`UsageSignal`.

This gate records an authorized lifecycle; it does not prove that exec
succeeded. After a normal successful exec, finalization may include changes made
by the target. Only when the child dies after `Armed` but before either CLOEXEC
exec closure or complete observation of `Failed` is pre-exec death
indistinguishable from exec closure. In that narrow false-positive case, no
target ran and finalization can include at most the already validated, fully
materialized unchanged projection. The gate does not provide signal forwarding
or prevent a supervisor from orphaning an executed target.

The older-kernel `SO_PEERCRED + /proc` peer-auth path is available only when an
operator disables the default hardened posture. A failure while taking the
`SO_PEERPIDFD` path must reject the peer, not silently switch to the weaker
path.

### 7. The runtime API cannot name storage keys

The daemon-to-backend wire exposes only:

- `POST /v1/wrappers`
- `POST /v1/wrapper-invocations`
- `POST /v1/wrapper-invocations/finalize`

Requests carry wrapper and resource names. No runtime DTO accepts a store id,
`resources/<id>` key, `control/*` key, slot path, or arbitrary storage prefix.
The backend derives the store and principal from mTLS.

### 8. The backend authorizes persistence

The target sees writable files and libsecret surfaces regardless of the
permission's writable flag. This preserves normal application behavior such as
write-then-fallback credential helpers.

Persistence is separate:

- the Checkout response carries a backend-derived writable hint;
- `underwrap-ns` discards changed bytes immediately when that hint is false;
- a submitted Checkin is translated to finalize;
- the backend re-evaluates the permission and writable flag;
- the backend rejects a submitted env section that differs from the resource
  snapshot loaded during finalize.

A permission that becomes read-only during an invocation rejects the final
writeback. Resource mutation is a whole-blob, last-writer-wins operation rather
than a transaction: a concurrent admin edit can race between finalize's load
and save. Operators must serialize edits to one resource; the comparison is not
a compare-and-swap guarantee.

## Local wrapper boundary

### Instance layout

Each frontend instance has one:

- daemon user and primary group named `underwrap-<instance>`;
- setgid entry point `/run/wrappers/bin/underwrap-<instance>`;
- socket `/run/underwrap-<instance>/wrapper.sock`;
- mTLS credential and state directory;
- systemd service.

Per-user shims call:

```text
underwrap-<instance> <wrapper> [args...]
underwrap-<instance> debug <wrapper> [args...]
```

The shims are unprivileged convenience scripts.

### Effective-GID authentication

`SO_PEERCRED` reports real credentials, while the security wrapper changes the
effective GID. On Linux 6.5+, `underwrapd` requests a pidfd with
`SO_PEERPIDFD`, reads the peer's status while holding that pidfd, cross-checks
identity, and compares the effective GID with the configured instance group.
Holding the pidfd prevents PID recycling between lookup and credential read.

The default NixOS posture asserts Linux 6.5+. When an operator disables that
posture on an older kernel, the daemon can use `SO_PEERCRED` plus
`/proc/<pid>/status`; this is a weaker, explicitly selected claim.

### Wrapper selection and target authority

After the peer passes the instance gate, the daemon:

1. validates the wrapper name against root-owned config;
2. resolves its one resource;
3. forwards wrapper, resource, and variant to the backend;
4. waits for policy authorization;
5. chooses the configured real target or the debug tool.

A forged `Real` variant cannot select the debug tool, because target selection
occurs in the daemon after the backend's variant decision.

### Stage-1 privilege drop

Before executing the namespace stage, `underwrap-exec` attempts
`setgroups(0, NULL)` and rejects errors other than `EPERM`. A setgid-only process
normally lacks `CAP_SETGID`, so `EPERM` can leave the caller's supplementary
groups in stage 1. It then performs and checks:

1. `setresgid(real_gid, real_gid, real_gid)`;
2. `setresuid(real_uid, real_uid, real_uid)`.

Real, effective, and saved IDs are restored together, so the instance-group
identity cannot be regained through a saved GID. The capability-bearing
namespace stage replaces supplementary groups with the dedicated target group
before target execution; the final target does not retain caller groups.

## Target isolation

### Dedicated UID boundary

Every `(instance, wrapper)` has a distinct `underwrap-target-*` UID/GID. Two
wrappers that execute the same binary still have different target UIDs. The
same wrapper name in two instances also has different target UIDs. A wrapper's
real and debug variants share its target UID because they operate on the same
resource trust domain.

The target child:

- restricts supplementary groups to the target's own group;
- calls `setresgid` and `setresuid` for all ID slots;
- sets `PR_SET_NO_NEW_PRIVS`;
- clears ambient, permitted, effective, and inheritable capabilities;
- verifies the complete ID switch and verifies that a return to UID 0 fails.

For env-only runs, the namespace supervisor drops all capabilities before it
waits. For files or libsecret runs, it retains `CAP_SYS_ADMIN`, `CAP_SETUID`, and
`CAP_SETGID` while the target runs and through the bounded post-run fsuid window,
then drops them. `CAP_SETPCAP` is named on the NixOS security wrapper only so the
wrapper can raise the other capabilities; NixOS does not pass it to
`underwrap-ns`. The supervisor does not selectively discard the now-unneeded
`CAP_SYS_ADMIN` before waiting.

### `/proc` access classes

Linux exposes process state through different ptrace access modes:

- READ-mode files such as `environ`, `maps`, `smaps`, `auxv`, and descriptor
  links under `fd`, `cwd`, `root`, and `exe` use `ptrace_may_access` with a UID
  check. The caller and target UIDs differ, so the check fails across the target
  subtree.
- ATTACH-mode access such as `/proc/<pid>/mem`, `ptrace`, and
  `process_vm_readv` is additionally blocked by Yama `ptrace_scope=2` for
  unprivileged callers.
- world-readable metadata such as `cmdline`, `comm`, `stat`, `status`, cgroup,
  limits, and mount metadata remains visible. Secrets never travel in argv, and
  mountpoint names are not treated as secret.

Yama is defense in depth for ATTACH-mode vectors. It is not the mechanism that
protects `environ`; the dedicated UID is.

### Namespaces

Every target uses fresh PID, mount, IPC, and UTS namespaces.

- The target becomes PID 1 and sees a fresh `/proc` containing only its
  namespace.
- When PID 1 exits, the kernel terminates remaining namespace processes.
- Private mount propagation keeps files/libsecret mounts out of the host mount
  namespace.
- A separate IPC namespace prevents access to the caller's SysV IPC and POSIX
  message queues.

Namespaces limit visibility and lifecycle; they do not replace the UID boundary
for host-side `/proc` confidentiality.

### Target-to-caller hardening

Before target exec, `underwrap-ns`:

- starts a new session with `setsid`, removing the controlling-terminal
  relationship while preserving stdio;
- joins a fresh anonymous session keyring when kernel keyrings are available;
- marks every inherited fd above stderr close-on-exec.

These controls reduce accidental reach back into the caller, but underwrap does
not claim to be a complete adversarial target sandbox.

## Environment and process state

### Cleared environment

The target starts with `env_clear()`. underwrap then adds:

1. a built-in passthrough set;
2. additional names from `envPassthrough`;
3. fixed trusted values from `envSet`;
4. env-slot values from the resource.

The NixOS module rejects `LD_*` values, rejects caller passthrough of forced
home/XDG variables, and prevents a per-wrapper `XDG_RUNTIME_DIR` override.
`PATH` is not inherited unless an operator sets a trusted fixed value.

### Home and runtime paths

`HOME` defaults to the original caller's passwd home path. The target runs as a
different UID and therefore cannot traverse caller-owned directories merely
because `HOME` names them. Use `accessPaths` for specific required directories.

`XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, and `XDG_STATE_HOME`
default under the effective `HOME`. `XDG_RUNTIME_DIR` points to a dedicated
`0700` directory owned by the target UID under
`services.underwrap.targetRuntimeDirBase`.

The caller's `/run/user/<uid>` tree is not mounted automatically.

### Fixed built-in passthrough

The built-in set includes locale/terminal values plus caller-controlled values
such as:

- `TMPDIR`;
- `SSH_AUTH_SOCK`;
- `DISPLAY` and `WAYLAND_DISPLAY`;
- HTTP/HTTPS proxy variables.

`envPassthrough` can add names but cannot remove the built-in set per wrapper.
Treat this as a first-release limitation when assessing a narrow helper. Passing
a path-valued variable does not bypass normal DAC or namespace visibility, but
it can still steer behavior when the referenced path or socket is reachable.

### Dumpability, core dumps, and zeroization

`underwrap-exec`, `underwrap-ns`, and `underwrap-secretsd` set
`PR_SET_DUMPABLE=0` before handling secret material. The final target exec can
reset dumpability; target confidentiality therefore relies on its dedicated UID,
not on dumpability alone.

`underwrap-ns` sets both soft and hard `RLIMIT_CORE` to zero before target exec.
Secret buffers are zeroized after handoff where practical.

## Resource projections

Every resource is one JCS-canonical JSON blob containing env, files, and
libsecret sections. The daemon treats it as opaque canonical bytes and caches it
only for the scoped resource channel.

### Env

Env entries are inserted into the target environment. Runtime finalize compares
the submitted env section with the resource snapshot it loads and rejects a
difference; the admin surface is the intended env editor. The compare and
whole-blob save are not serialized, so a stale runtime writeback can overwrite
an admin edit that races between them. Do not mutate one resource concurrently.

Env mode gives the target the raw value for its lifetime. Use it only for a
helper that has been reviewed for output, logging, persistence, plugins, helper
execution, and caller-controlled arguments.

### Files

For a files-enabled wrapper, `underwrap-ns`:

- after Checkout, decodes the blob and purely validates every configured mount
  and file key before namespace preparation, idmapped-mount realization, PID
  fork, staging, target-UID switch, or target-facing helper launch;
- requires each clean file key to route to exactly one strict mount prefix,
  rejects a key equal to a mount path, and rejects file/descendant collisions;
- mounts a namespace-private tmpfs with `noswap`, `mode=0700`, and a size cap;
- realizes only the completed routing plan and materializes regular files from
  the blob;
- bind-mounts configured clean relative paths into the caller-home view;
- collects only regular files after an authorized lifecycle and rejects
  symlinks.

The target sees the mounts as writable. Final persistence follows the permission
writable flag and backend finalize checks.

### Explicit caller-owned paths

`accessPaths` grants the dedicated target UID access to selected caller-owned
directories through idmapped bind mounts. The mapping contains exactly one UID
and GID pair: caller ↔ target. Target writes land on the underlying filesystem
owned by the caller; other file owners remain unmapped.

The named directory's filesystem must support idmapped mounts. Nested submounts
are handled independently: supported submounts are idmapped, and unsupported
ones are skipped with a warning. The mounts exist only in the private mount
namespace.

Creating the idmap user namespace requires a short-lived helper whose proc map
files the parent can write. After fd-20 authentication, `underwrap-exec`'s
caller-sanitized hint causes this helper to be forked **before** fd 3 or Checkout
is read. The helper closes fd 3 and fd 20 before briefly making itself dumpable;
the secret-bearing parent remains non-dumpable. The decoded bundle must agree
with the hint or the invocation aborts.

Grant only the directories the target needs. An access path is host access, not
backend-managed secret storage.

### Libsecret

Libsecret mode uses two distinct channels:

1. fd 20 remains between `underwrap-ns` and `underwrapd` for Checkout and final
   UsageSignal/Checkin;
2. a one-shot anonymous pipe carries only the initial libsecret section from
   `underwrap-ns` to `underwrap-secretsd`.

Inside the namespace:

- a private dbus-daemon listens on a filesystem socket in a private tmpfs;
- secretsd loads the canonical initial section into memory;
- the target receives the private `DBUS_SESSION_BUS_ADDRESS`;
- every successful mutation is atomically reflected in a canonical tmpfs state
  file before the D-Bus acknowledgement returns;
- the target's exit terminates dbus-daemon and secretsd;
- the namespace supervisor reads final state and submits writeback through fd
  20.

secretsd never holds the daemon resource channel. The target never inherits it.

The private filesystem socket and mount namespace keep normal NixOS path-based
session buses out of the target's view. Abstract Unix sockets are not hidden by
the mount namespace; a deliberately nonconforming client that ignores
`DBUS_SESSION_BUS_ADDRESS` could attempt to reach an abstract host bus if one
exists. This is a defense-in-depth limitation, not part of the normal NixOS
session-bus layout.

See [secretsd security](secretsd-security.md) for input validation, state caps,
and object-path rules.

## Backend security

### Runtime mTLS

`underwrap-keygen` creates a self-signed client leaf and private key on the
frontend host. An operator registers the certificate PEM in the admin UI. The
backend stores its SHA-256 fingerprint.

At TLS time, the backend verifies proof of possession of the presented leaf's
private key. Runtime principal resolution then requires an active client record
with the same fingerprint and maps that client to one store. Certificate
subject, SAN, CA chain, KU/EKU, and validity dates do not define runtime
identity.

The frontend admits only an absolute HTTPS backend URL with a host. The NixOS
module rejects non-`https://` instance options, daemon startup validates the URL
before credential reads or wrapper-socket binding, `BackendClient::open` repeats
the check before file I/O or worker creation, and reqwest is configured
HTTPS-only so redirects cannot downgrade the runtime channel to plaintext. A
configured CA bundle is the complete server trust set for that runtime client
instance and excludes bundled public WebPKI roots; when no bundle is configured,
the frontend uses reqwest's bundled WebPKI roots. Normal certificate hostname
verification applies in either mode. This server trust selection is separate
from the client-certificate fingerprint that defines runtime identity, and the
URL rule does not change either mechanism.

The checked value is the endpoint the daemon dials. Direct backend rustls and
Layer-4 TLS passthrough preserve the mTLS peer certificate. An HTTPS Layer-7
proxy address is syntactically admitted, but ordinary TLS termination followed
by HTTP does not preserve the backend's peer-certificate principal. No trusted
forwarded-certificate or proxy-identity mechanism is implied.

### Frontend daemon hardening

The NixOS service runs `underwrapd` as its per-instance user and group with:

- `NoNewPrivileges`;
- an empty capability bounding and ambient set;
- `ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, and `PrivateDevices`;
- namespace, kernel, control-group, personality, realtime, and W^X restrictions;
- a constrained syscall and address-family surface;
- read-only access to its state/config and write access only to its runtime
  directory.

The daemon needs cross-user `/proc/<pid>/status` access for effective-GID peer
authentication, so hiding other users' process entries would break its gate.

At startup, the daemon rejects a present runtime credential when:

- its file is owned by another UID;
- its mode is wider than `0600`;
- its parent directory is owned by another UID or wider than `0700`.

Missing credential files are allowed so enrollment can happen after service
startup; each wrapper request retries backend initialization.

### Admin authentication

The first operator is created only by the quiesced offline
`underwrap-backend admin bootstrap-user` command, executed with backend-host
root or service-identity filesystem authority. The public network surface
exposes only read-only setup status; it cannot create an operator. The command
reads the password from stdin, serializes independent processes with a stable
advisory lock, commits the user before the bootstrap sentinel, and never rolls
back a valid user when sentinel persistence fails.

The admin API and SPA share the backend HTTPS listener. Authentication uses:

- Argon2id password hashes;
- opaque `__Host-underwrap-session` cookies backed by an in-memory
  `tower-sessions` store;
- synchronizer-token CSRF with a bound cookie and `X-CSRF-Token` header;
- optional `allowed_origins` checks for state-changing requests;
- per-username login rate limiting and a dummy-hash verification path for
  unknown usernames.

Sessions do not survive backend restart. Password changes and user deletion
invalidate that user's sessions through `axum-login`'s session auth-hash check.
All authenticated users have full admin access; there are no roles or per-store
operator grants.

### Persistent stores

`PlainBackend` stores resource blobs as plaintext files owned by the backend
service account.

`EncryptedBackend` uses ChaCha20-Poly1305 with a 32-byte master key:

- the key is generated in memory when the store is created;
- the create response displays it once;
- no key file is written by underwrap;
- a reserved encrypted canary verifies later unlock attempts;
- an encrypted store starts locked after every backend restart;
- lock drops the in-memory backend and zeroizes the master key when its final
  reference is released;
- rotation re-encrypts and verifies a complete generation under a fresh key,
  displaces the live directory into a unique mode-`0700` backend-host recovery
  workspace, and promotes the staged generation with individually atomic
  renames;
- every displaced complete ciphertext generation remains unmodified while it is
  retained as a recovery artifact and is never automatically overwritten or
  deleted by rotation, restart, later rotation, response success, or browser
  acknowledgement;
- a failed post-swap rollback either restores the old canonical generation or
  locks the store with `rotation_recovery_required` while preserving every
  potentially useful directory;
- unlock and post-promotion reopen require the canonical directory to exist and
  never create an empty replacement after an interrupted swap.

The protected availability boundary runs from ciphertext-generation commit to
the one-time replacement-key response and external custody. The backend cannot
observe delivery or custody. Retaining every old-key-decryptable snapshot keeps
an externally held key useful after response loss; the confidentiality cost is
unbounded disk use and prolonged old-key access to stale ciphertext until
trusted host cleanup. Recovery paths are backend-host-only siblings and are not
exposed through admin or runtime APIs. The exact non-overwriting recovery,
verification, and selective-cleanup procedure is normative in
[Backend](backend.md#retained-rotation-generations).

The rotation mutex excludes another rotation only. Ordinary reads and writes
are not coordinated with the copy or multi-rename swap and can fail or be lost,
so operators must quiesce all access to the store during rotation. The procedure
does not claim CAS, response delivery, automatic reconciliation, or power-loss
/fsync durability.

The operator must keep displayed keys in an external password manager or vault.
Memory compromise of the running backend can recover an unlocked key; TPM/HSM
custody is out of scope.

## Permission model

Each store contains permission rows keyed by
`(runtime client instance, wrapper, resource)`. A row contains one permission
mode and one writable flag.

| Permission mode | Real variant | Debug variant |
| --- | --- | --- |
| `Locked` | deny | deny |
| `Unlocked` | allow | deny |
| active `UnlockedUntil` | allow | deny |
| expired `UnlockedUntil` | deny | deny |
| active `UnsafeDebugUntil` | allow | allow |
| expired `UnsafeDebugUntil` | deny | deny |

Expiry is evaluated lazily on every open and finalize. There is no background
sweeper and no per-invocation approval flow.

`UnsafeDebugUntil` selects `underwrap-debug-tool`, which can summarize the
environment, open an interactive shell, or execute an arbitrary command inside
the wrapper namespace. It receives the same projection and can submit writes
when the permission's writable flag is true. Use the shortest practical TTL and
return the permission to a safer mode immediately after the incident task.

The complete evaluation order and wire deny reasons live in
[Backend](backend.md#policy-model).

## Audit and diagnostics

Structured daemon events cover:

- peer-authentication failures and selected peer-auth path;
- wrapper invocation allows and denials;
- resource Checkout, Checkin, and UsageSignal activity;
- backend and channel failures.

Events contain wrapper/resource/principal metadata, never secret values or
resource blobs.

When an authorized lifecycle changes a files/libsecret projection that cannot
be persisted, `underwrap-ns` emits one structured `writeback_rejected` event
containing the resource id, resource name, and reason. Once the private gate
authorizes finalization, collection and canonical comparison still occur for
target exit 0, nonzero exit, and signal death. Finalization failures remain
diagnostic and do not replace the target's exit or signal status. It also
writes one human-readable stderr warning unless the caller set:

```text
UNDERWRAP_SUPPRESS_READONLY_WARNING=1
```

That variable is consumed by the wrapper and never passed into the target. If
the journald native socket is unavailable, the audit event falls back to a JSON
line on stderr even when the human warning is suppressed.

underwrap relies on service logs; it does not provide a persistent audit-ledger
product.

## Limitations and operational assumptions

Keep these limits explicit in deployment and wrapper reviews:

- Root or kernel compromise defeats the local boundary.
- Backend compromise exposes resources, policy, runtime identities, admin
  accounts, and unlocked encryption keys.
- The target can disclose its own projection.
- A broad helper, plugin system, shell escape, or overpowered token can make the
  wrapper broader than intended.
- Any local user can execute a system-wide instance entry point; backend policy
  does not identify the Unix caller.
- The built-in target environment passthrough set cannot be reduced per wrapper.
- Plain backend stores are plaintext at rest.
- Encrypted stores require an external key-custody process and manual unlock
  after restart.
- Browser sessions are memory-only and disappear on restart.
- Abstract D-Bus sockets are outside mount-namespace hiding.
- Resource and policy availability depends on the backend network path.
- `accessPaths` deliberately grants selected host-filesystem access to the
  dedicated target UID.
- The strongest peer-auth claim requires Linux 6.5+ and the default hardened
  posture.

For wrapper design, continue with the
[wrapper author checklist](wrapper-author-checklist.md). For deployment
mechanics, see [Architecture](architecture.md), [Configuration](configuration.md),
and [Testing](testing.md).
