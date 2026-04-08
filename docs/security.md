# Security

This document explains every security mechanism in underwrap, why it exists,
what alternatives were considered, and what limitations remain.

## Threat model

underwrap protects against:

1. **Same-user secret theft**: another process running as the same user
   reading the secret from the target's environment, memory, or `/proc`.
2. **Wrapper impersonation**: a process pretending to be a different wrapper
   to read a secret it shouldn't have access to.
3. **Secret persistence**: secrets remaining on disk, in logs, in core dumps,
   or in process memory longer than necessary.
4. **Privilege escalation**: the wrapper binary having more privilege than
   needed when executing the target.
5. **Cross-instance leakage**: one underwrap instance accessing another
   instance's secrets or tunnel.

underwrap does NOT protect against:

- **Root compromise**: root can always read process memory, files, and sockets.
- **Target process misbehavior**: once the target has the secret in its
  environment, it could leak it by spawning children or printing env.
- **Kernel compromise**: the security model depends on kernel namespaces,
  credentials, and capabilities.

## Design invariants

These rules are absolute. No feature, optimization, or future extension may
violate them. They exist because past design iterations made wrong assumptions
that would have silently degraded the security model.

### All secret material flows through StorageBackend

**Every byte of secret material** — whether injected via environment variable,
stored via libsecret, or derived via OAuth — must be read from and written to
the daemon's `StorageBackend`. No exceptions.

This means:
- No local JSON files containing secret values.
- No sidecar files, temp files, or caches containing secret values.
- No in-process databases (redb, sqlite) containing secret values.
- Metadata (attributes, labels, timestamps) is stored in `.meta` keys,
  secret values in separate `.secret` keys. Both go through
  `StorageBackend::write()` and `StorageBackend::read()`. Startup only
  reads `.meta` keys — secret bytes are never loaded until requested.

**Rationale**: The `StorageBackend` is the single enforcement point for
encryption at rest (encrypted backend), remote storage (remote backend),
and audit logging (daemon layer above the backend). Any secret that bypasses
it loses all three properties silently. A plain JSON file on disk is an
unencrypted, unaudited, local copy of secret material — exactly what the
remote backend is designed to prevent.

### Running a wrapper does not expose the raw secret

A user who executes a wrapper binary can *use* the secret through the fixed
target binary (e.g., `work-gh-authorized pr list` uses the GitHub token via
`gh`). But the user **cannot read the raw secret value** through underwrap's
mechanisms:

- The secret passes from daemon → `underwrap-exec` → pipe fd 3 →
  `underwrap-ns` → target env. The user never holds it.
- The wrapper processes that briefly hold the secret set
  `PR_SET_DUMPABLE=0` and never `exec()` again, so the flag persists.
  This blocks `/proc/<pid>/mem`, `/proc/<pid>/environ`, and `ptrace`
  from same-UID processes (requires `CAP_SYS_PTRACE` to bypass;
  see `ptrace(2)` access mode algorithm step 4).
- The target binary path is locked in a root-owned sidecar config
  (`/etc/underwrap/wrappers/`). The user cannot substitute a binary
  that exfiltrates the secret.

Whether the target binary *itself* offers a way to print the secret (e.g.,
`gh auth token`) is outside underwrap's control — the choice of target
binary is part of the trust model, made by the administrator in the NixOS
configuration.

### Authenticated channels only

Any communication channel that carries secret material must be authenticated
at creation time. Authentication means: the daemon verified the peer's
identity via kernel-provided credentials (SO_PEERCRED effective GID) before
the channel was established. No tokens, no passwords, no environment
variables.

Channels that carry secrets:
- **Read socket connection**: authenticated by SO_PEERCRED effective GID.
- **Pipe fd 3** (underwrap-exec → underwrap-ns): inherited through fork,
  no filesystem path, no way for outside processes to access it.
- **Secretsd channel fd** (libsecret mode): `socketpair()` created during
  the GID-authenticated read request, one end kept by the daemon, the other
  inherited through the fork chain. Anonymous — no filesystem path.

### File descriptors carrying secrets must not leak

Any fd that carries secret material or provides an authenticated channel to
the daemon must be closed before exec'ing untrusted processes. Specifically:

- Pipe fd 3 is consumed (read + closed) by `underwrap-ns` before exec'ing
  the target.
- The secretsd channel fd must be closed before exec'ing the target. Only
  `underwrap-secretsd` (forked before the target) should hold it.

### Processes holding secrets must be non-dumpable

Any process that holds secret material in memory must call
`prctl(PR_SET_DUMPABLE, 0)`. This blocks `/proc/<pid>/mem`,
`/proc/<pid>/environ`, `/proc/<pid>/fd/`, and `ptrace` access from
same-UID processes without `CAP_SYS_PTRACE`.

Critical detail: `exec()` of a normal binary (no setuid/setgid/capabilities)
**resets dumpable to 1** (`prctl(2)` man page: "Normally, the dumpable
attribute is set to 1"; it is only reset to `suid_dumpable` when credentials
change). Therefore:

- Wrapper parent processes (`underwrap-exec`, `underwrap-ns`) set
  `PR_SET_DUMPABLE=0` and **never exec again** — the flag persists.
- `underwrap-secretsd` is exec'd, so it starts dumpable. It **must** call
  `PR_SET_DUMPABLE=0` as its first action, before reading any secrets from
  the channel. Between exec and that `prctl` call there is a brief window,
  but no secrets have been read yet, and no other processes in the namespace
  have started.
- The target process is exec'd and becomes dumpable. It is protected by
  the PID namespace (defense in depth, not sole protection — see
  limitations below).

## Wrapper identity: setgid + effective GID

### The problem

Any process that can connect to the daemon's read socket could claim to be
any wrapper and read its secret. The wrapper name in the protocol message is
caller-controlled and trivially spoofable.

### The solution

Each wrapper binary is installed `setgid` to a dedicated Unix group (e.g.
`underwrap-myinstance-gh-authorized`). When the wrapper connects to the daemon,
the kernel records the effective GID. The daemon reads it from
`/proc/<pid>/status` and maps it to a wrapper name.

The client-supplied wrapper name in the request is ignored entirely. Identity
comes from the kernel.

### Why not these alternatives?

| Approach | Problem |
|----------|---------|
| Trust client-supplied wrapper name | Any connected process could claim any name |
| Check `/proc/<pid>/exe` | All wrappers are copies of the same binary |
| Per-wrapper sockets | More complex, equally secure |
| Environment variable for identity | Caller-controlled, trivially spoofable |

### Why effective GID, not real GID?

`SO_PEERCRED.gid` returns the *real* GID, not the effective GID. After
`setgid`, the effective GID changes but the real GID stays the same. The
daemon reads the effective GID from `/proc/<pid>/status` (the `Gid:` line,
second field).

### Why NixOS security.wrappers?

`security.wrappers` handles setgid binary installation correctly: it mounts
`/run/wrappers` without `nosuid` and creates a small C helper that sets
credentials before exec.

### Wrapper identity from effective GID, not argv[0]

`underwrap-exec` derives the wrapper name from its own effective GID
(`getegid()` → `getgrgid()` → strip `underwrap-` prefix), NOT from
`argv[0]`. This prevents symlink confused-deputy attacks where a user
creates a symlink from one wrapper name to another wrapper's binary:

```
~/bin/wrapper-B -> /run/wrappers/bin/wrapper-A
# Kernel: GID = A's group
# argv[0] = "wrapper-B" (user-controlled, not trusted)
# getegid() = A's GID → group "underwrap-A" → wrapper name "A"
# Sidecar config loaded: A's config (correct, not B's)
```

The effective GID is set by the kernel from the setgid binary and cannot
be influenced by the caller. This binds identity and policy to the same
trusted source.

## PID namespace isolation

### The problem

After the wrapper drops privilege and execs the target, the target is
"dumpable" — the kernel resets the dumpable flag when there's no credential
mismatch at exec time (`euid==uid`, `egid==gid`). If `suid_dumpable` is 0
(the default) and the exec involves no credential change, dumpable is set
to 1 (SUID_DUMP_USER).

### Why PR_SET_DUMPABLE doesn't work for the target

We set `PR_SET_DUMPABLE=0` in the child before exec, but `exec()` of the
target binary (a normal binary without setuid/setgid/capabilities) resets
dumpable to 1.

### What actually protects the target

The VM test proves that same-UID host processes **cannot** read
`/proc/<host-pid>/environ` of the final target, even though:
- The target has a host-visible PID (root can find it)
- The target is dumpable (exec reset it to 1)
- The attacker has the same UID

The protection comes from **Yama LSM** (`kernel.yama.ptrace_scope=1`),
which is the default on NixOS (and most modern Linux distributions).
With `ptrace_scope=1`, `/proc/<pid>/environ` access (which requires
`PTRACE_MODE_READ_FSCREDS`) is restricted to **descendant processes only**.
A same-UID process that is not a descendant of the target is denied.

The PID namespace provides **defense in depth**:
- Makes the target harder to discover (different PID space)
- When combined with a mount namespace (libsecret mode), a fresh `/proc`
  mount ensures processes inside the namespace can only see namespace-local
  PIDs

But the primary protection for the target's environment is Yama, not the
PID namespace.

### Dependency on Yama

If `kernel.yama.ptrace_scope` is set to 0 (disabled), same-UID processes
**can** read the target's `/proc/<pid>/environ`. NixOS sets `ptrace_scope=1`
by default. The VM test verifies this protection holds.

Systems that set `ptrace_scope=0` should not be considered safe for
env-mode secret injection without additional hardening.

### Why a two-stage wrapper?

NixOS `security.wrappers` doesn't allow both `setgid` and `capabilities`
on the same binary — they're mutually exclusive. So the wrapper is split:

- **underwrap-exec** (stage 1, setgid): gets secret from daemon, drops
  privilege, pipes secret to stage 2 via fd 3.
- **underwrap-ns** (stage 2, `cap_sys_admin`): creates PID namespace,
  drops capabilities, execs target.

The secret passes between stages via a pipe on file descriptor 3 — never in
environment variables (visible in `/proc/pid/environ`), command-line arguments
(visible in `/proc/pid/cmdline` and `ps`), or the filesystem (visible to any
process with directory access). fd 3 was chosen because fd 0/1/2 are
stdin/stdout/stderr which must flow through to the target untouched.

### Why not user namespaces (CLONE_NEWUSER)?

Unprivileged user namespaces (`CLONE_NEWUSER | CLONE_NEWPID`) would avoid
needing `cap_sys_admin`. But after a setgid credential change, writing
`/proc/self/uid_map` fails with EPERM. The setgid identity mechanism is
incompatible with user namespace creation.

## Wrapper process memory protection

### The problem

The two wrapper parent processes briefly hold the secret in memory while
piping it. They're in the parent namespace and visible to same-UID processes.

### The solution

Both `underwrap-exec` and `underwrap-ns` call `prctl(PR_SET_DUMPABLE, 0)`
immediately after receiving the secret. Since neither process execs again
(they fork and waitpid), the dumpable flag persists for their entire
lifetime. This blocks:

- `/proc/<pid>/mem` reads by same-UID processes
- `/proc/<pid>/environ` reads by same-UID processes
- `ptrace` attach from same-UID processes

Both processes also zeroize the secret `String` from memory after piping it.

## Core dump prevention

`underwrap-ns` sets `RLIMIT_CORE=0` in the child before exec. If the target
crashes, no core dump is written — regardless of system-wide settings.

The VM test verifies this: core dumps are enabled globally, the target
receives SIGSEGV, and no core file appears.

## Privilege drop

`underwrap-exec` drops all elevated privileges before any secret touches
the target process:

1. `setgroups(0, NULL)` — drops supplementary groups
2. `setgid(real_gid)` — drops effective GID
3. `setuid(real_uid)` — drops effective UID

All return values are checked. Failure aborts the wrapper — executing the
target with elevated privilege is worse than not executing at all.

## Config directory trust

`underwrap-exec` always uses the hardcoded path `/etc/underwrap/wrappers/`
for sidecar configs. There is no environment variable or command-line
override — this prevents config redirection attacks where an attacker
could point the wrapper at a malicious config with an attacker-controlled
target binary.

The target path in the sidecar config must be absolute — relative paths
are rejected.

This prevents an attacker from pointing the wrapper at an attacker-controlled
config that redirects the target to a binary that exfiltrates the secret.

## Environment sanitization

The target process starts with a **clean environment** (`env_clear()`).
It does not inherit the user's shell environment wholesale. This blocks:

- `LD_PRELOAD` / `LD_LIBRARY_PATH` injection
- User injection of internal `UNDERWRAP_*` control variables
- Arbitrary inherited env vars that could alter target behavior

The following are passed through from the user's environment (default
whitelist):

- Identity: `HOME`, `USER`, `LOGNAME`, `SHELL`
- Terminal: `TERM`, `COLORTERM`
- Locale: `LANG`, `LC_*`, `TZ`
- XDG: `XDG_RUNTIME_DIR`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`,
  `XDG_CACHE_HOME`, `XDG_STATE_HOME`
- System: `PATH`, `TMPDIR`, `SSH_AUTH_SOCK`
- Display: `DISPLAY`, `WAYLAND_DISPLAY`
- Proxy: `http_proxy`, `https_proxy`, `no_proxy` (and uppercase)

Additional passthrough vars and fixed env vars can be configured per
wrapper in the NixOS module (`envPassthrough`, `envSet`). These come
from the root-owned sidecar config, not from the user.

If a wrapper should not rely on `PATH`, it should call downstream tools by
absolute Nix store path. This is the recommended pattern for agent-facing
wrappers.

Internal env vars (`UNDERWRAP_LIBSECRET`, `UNDERWRAP_DBUS_DAEMON`,
`UNDERWRAP_SECRETSD`, `UNDERWRAP_ENV_CONFIG`) are always stripped from
the user's environment before being set from the trusted sidecar config.
A malicious user cannot inject these to force libsecret mode or substitute
binary paths.

## Socket permissions

- **Read socket** (0666): world-connectable. Authorization happens inside
  the daemon via effective GID verification. A process without a wrapper GID
  is rejected. The daemon never falls back to trusting the client name.
- **Control socket** (0600): daemon user only.
- **Runtime directory** (0775): group-writable so the tunnel user can create
  the forwarded backend socket.

## Immutability enforcement

Secrets declared `mutable: false` cannot be overwritten after initial storage.
A per-secret write lock (HashSet mutex) prevents TOCTOU races where two
concurrent writers both observe "absent" and both succeed.

## OAuth hardening

The token exchange always uses a hardcoded Google endpoint. The stored
`token_uri` field is ignored, preventing SSRF / credential exfiltration via
tampered secret material. The HTTP client has no-redirect policy. Error
response bodies are truncated to 200 characters.

## Audit trail

Every security-relevant operation is logged:

| Event | Level | Fields |
|-------|-------|--------|
| `read_granted` | info | wrapper, secret, peer_uid, scope |
| `read_denied` | warn | wrapper, reason, peer_uid, scope |
| `secret_stored` | info | secret, peer_uid |
| `store_denied` | warn | secret, reason, peer_uid |
| `unlocked` | info | wrapper, scope, seconds, peer_uid |
| `locked` | info | target, scope, peer_uid |
| `auth_denied` | warn | socket, reason, peer_uid, peer_gid |

Secret material is never logged. The VM test verifies this by grepping
journald for known secret values.

## Unlock model

Wrappers with `requireUnlock = true` deny reads until explicitly unlocked.

- **Time-based**: unlock for N seconds, auto-lock on expiry.
- **UID-bound**: grants keyed by `(wrapper, scope, uid)` — one user's
  unlock can't be reused by another.
- **Root admin grants**: root creates grants with `uid=None` covering all UIDs.
- **Self-service SSH challenge**: users unlock via the read socket. The
  daemon issues a nonce, the wrapper signs it with the SSH agent, the daemon
  verifies against the configured public key.

## Remote backend isolation

In remote mode, secrets never exist on the target machine. A backend daemon
runs on the operator's workstation. An SSH tunnel forwards its socket to the
target. When the tunnel disconnects, the socket disappears and all reads fail.

The tunnel user (`underwrap-<instance>-tunnel`) has no shell and is restricted
to socket forwarding via `authorized_keys`.

## Instance isolation

Each named instance gets fully isolated resources: separate daemon user,
daemon group, tunnel user, per-wrapper groups, runtime directory, state
directory, and systemd service. No group membership overlap between instances.

## Master key zeroization

The encrypted backend's master key is wrapped in `ZeroizeOnDrop`. Raw key
file bytes are zeroized after copying into the key struct.

## Secretsd channel (libsecret mode)

### The problem

In libsecret mode, `underwrap-secretsd` runs inside the PID namespace and
needs to read/write secrets through the daemon for the lifetime of the
target process. But secretsd runs as the regular user — it doesn't have the
wrapper's setgid GID, so it can't authenticate to the daemon's read socket.
It needs a pre-authenticated, persistent, bidirectional channel.

### The solution: socketpair() created during GID-authenticated request

When a libsecret-enabled wrapper connects to the daemon, the daemon:

1. Verifies the wrapper's identity via SO_PEERCRED effective GID (standard
   read-socket authentication).
2. Creates a `socketpair(AF_UNIX, SOCK_STREAM, 0)`.
3. Sends one end back to `underwrap-exec` via `SCM_RIGHTS` fd passing,
   alongside the normal secret response.
4. Keeps the other end and spawns a scoped handler task that only permits
   read/write of that wrapper's libsecret secret.

The socketpair fd is then inherited through the fork/exec chain:

```
underwrap-exec (receives fd via SCM_RIGHTS during GID-authenticated request)
  └─ fork → child inherits fd
       └─ exec underwrap-ns (fd survives exec, no FD_CLOEXEC)
            ├─ fork → secretsd (inherits fd, uses it for daemon I/O)
            ├─ close fd  ← CRITICAL: must happen before target exec
            └─ exec target (does NOT have the fd)
```

### Why this is secure

**Anonymous socket — no discovery path.** `socketpair()` creates connected
sockets with no filesystem path and no abstract name (`unix(7)`: "Sockets
created by socketpair(2) are anonymous"). The only way to obtain the fd is
to inherit it through `fork()` or receive it via `SCM_RIGHTS`.

**Authenticated by construction.** The daemon creates the socketpair during
a GID-verified request. The channel is born authenticated — there are no
credentials to steal, replay, or forge. The daemon handler on its end knows
which wrapper this channel belongs to and scopes all operations to that
wrapper's libsecret secret.

**Lifetime-bound.** When the PID namespace's init process exits, the kernel
sends `SIGKILL` to all remaining processes in the namespace
(`pid_namespaces(7)`). Secretsd dies, its fd closes, the daemon detects EOF
and tears down the handler. No cleanup race, no leaked channels.

**secretsd is non-dumpable.** After exec, secretsd calls
`prctl(PR_SET_DUMPABLE, 0)` before reading secrets from the channel. This
blocks `/proc/<pid>/fd/` access from same-UID processes: reading symlinks
in `/proc/<pid>/fd/` requires passing `ptrace_may_access()` with
`PTRACE_MODE_READ_FSCREDS`, which checks dumpable at step 4 and denies
access without `CAP_SYS_PTRACE` (`ptrace(2)`).

**Target cannot access the fd.** The secretsd channel fd is closed before
the target is exec'd. Even if the target could discover secretsd's PID,
`/proc/<secretsd-pid>/fd/` access is blocked by `PR_SET_DUMPABLE=0`. The
target interacts with secrets only through the D-Bus Secret Service API on
the private bus.

### Daemon-side handler scoping

The daemon handler for a secretsd channel:

- Allows read/write of exactly one secret: `<wrapper-name>/libsecret`.
- Rejects all other secret IDs.
- Terminates when the socketpair is closed (EOF).

This scoping is structural, not policy-based — the handler is created with
the wrapper name baked in. A compromised secretsd can only access the secret
it was authorized for.

### Concurrent invocations

Two simultaneous invocations of the same wrapper each get their own
socketpair and daemon handler, both scoped to the same secret ID. The
`StorageBackend` provides atomic writes via `tempfile::persist()`, so
the last write wins. This is a consistency concern for rare edge cases,
not a security issue.

## Libsecret mode: private D-Bus bus

### The problem

Tools like `gh` use libsecret to store/retrieve credentials via the system's
`org.freedesktop.secrets` D-Bus service. If the target connects to the real
session bus, it would bypass underwrap's isolation entirely — reading from
and writing to the system keyring.

### The solution

`underwrap-ns` launches a private `dbus-daemon` on a Unix socket inside
a namespace-private tmpfs mount. `underwrap-secretsd` connects to this
private bus and claims `org.freedesktop.secrets`. The target's
`DBUS_SESSION_BUS_ADDRESS` points to the private bus.

All libsecret operations go to our secretsd, which routes secret values
through the daemon via the authenticated socketpair channel.

### Mount namespace isolation

When libsecret mode is enabled, `underwrap-ns` creates a mount namespace
(`CLONE_NEWNS`) in addition to the PID namespace. Inside the namespace:

1. **All mounts made private** (`MS_PRIVATE | MS_REC` on `/`) to prevent
   bind mounts from propagating back to the parent namespace via shared
   mount propagation (the default on systemd-based systems).

2. **Fresh `/proc` mounted** so the PID namespace is properly isolated.
   Without this, the target would see the host's `/proc` with all host
   PIDs visible. With the fresh mount, only namespace-local processes
   are visible. **Failure is fatal.**

3. **Private tmpfs for the bus socket.** The dbus-daemon socket is created
   on a tmpfs mounted inside the namespace at `/tmp/.underwrap-ns-bus/`.
   This path does not exist in the parent namespace — the socket is
   completely unreachable from outside. **Failure is fatal.**

4. **Real session bus socket hidden** by bind-mounting `/dev/null` over
   `/run/user/<uid>/bus`. **Failure is fatal** if the socket exists.

All mount operations are hard failures (abort, not warn). This follows
the project's no-fallback rule: if a security mechanism fails, the
wrapper does not execute.

**Limitation**: D-Bus abstract sockets (name starts with `\0` in `sun_path`)
live in the network namespace, not the filesystem. They have no file path,
so bind mounts cannot hide them. If the real session bus uses an abstract
socket (e.g., `unix:abstract=/tmp/dbus-XXXXX`), a target that ignores
`DBUS_SESSION_BUS_ADDRESS` could potentially connect to it. Blocking
abstract sockets would require a network namespace (`CLONE_NEWNET`), which
would also cut off the target's network access entirely.

Modern systemd creates the session bus at `unix:path=/run/user/<uid>/bus`
(a filesystem socket, which we hide). `DBUS_SESSION_BUS_ADDRESS` is always
set, so all well-behaved libsecret clients use our private bus. This is a
defense-in-depth gap (theoretical, not practical) that is inherent to the
Linux socket model.

## Dependency advisories

**RUSTSEC-2023-0071** (rsa 0.9.10, medium severity): Marvin Attack timing
side-channel in RSA decryption. Pulled in via `ssh-key` → `underwrapd`.

**Not exploitable in this project.** We use the `rsa` crate only for RSA SSH
signature **verification** during the unlock challenge-response flow. The
Marvin Attack targets RSA **decryption** (PKCS#1 v1.5 unpadding), which
requires the private key. Our daemon never holds RSA private keys and never
performs RSA decryption. No fixed version is available upstream.

**Why no fix exists (as of 2026-04):** The RustCrypto `rsa` crate's fix
(tracked in [RustCrypto/RSA#390](https://github.com/RustCrypto/RSA/issues/390))
depends on `crypto-bigint` implementing constant-time modular exponentiation.
That work is in progress but has no release date. The crate is still pre-1.0.
There is no alternative pure-Rust RSA implementation to switch to — the
`ssh-key` crate (the standard Rust SSH key parser) depends on `rsa` for RSA
key support.

**Why we keep RSA support:** Users with legacy RSA SSH keys need to be able
to use the unlock challenge-response flow. Dropping RSA would be a
functionality regression. Since the vulnerable code path (decryption) is
never invoked, the advisory is a dependency-tree artifact, not a real risk.

Audit performed with `cargo-audit` 0.22.0 against the RustSec advisory DB.
Last checked: 2026-04-07.

## Rejected approaches

| Approach | Why rejected |
|----------|-------------|
| Trust client-supplied wrapper names | Trivially spoofable |
| Trust `argv[0]` for wrapper identity | Symlink confused deputy: user pairs wrong secret with wrong target. Use egid instead. |
| `/proc/<pid>/exe` identity check | All wrappers are the same binary |
| `PR_SET_DUMPABLE` on target | Kernel resets after exec without credential mismatch |
| `suid_dumpable=0` sysctl | Same exec-time reset issue |
| User namespaces | EPERM after setgid credential change |
| Combined setgid + capabilities | NixOS security.wrappers rejects this combination |
| Fallback to direct exec | Silent security degradation |
| Per-wrapper dedicated UIDs | Target can't access user's files |
| `memfd_create` injection | Unnecessary given PID namespace isolation |
| stdin/tempfile injection | Unnecessary given PID namespace isolation |
