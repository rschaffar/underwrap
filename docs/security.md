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
credentials before exec. The source binaries are copies (not symlinks) so
`argv[0]` contains the wrapper name for loading the correct sidecar config.

## PID namespace isolation

### The problem

After the wrapper drops privilege and execs the target, the target is
"dumpable" — the kernel resets the dumpable flag when there's no credential
mismatch at exec time (`euid==uid`, `egid==gid`). Same-UID processes can
read `/proc/<pid>/environ` and extract the secret.

### Why PR_SET_DUMPABLE doesn't work for the target

We set `PR_SET_DUMPABLE=0` in a `pre_exec` hook, but `exec()` resets it
because there's no credential mismatch (we already dropped privilege).
The `suid_dumpable` sysctl doesn't help either — the flag is reset based
on credential state at exec time, not the sysctl value.

### The solution

The target runs inside a PID namespace created by `underwrap-ns`. Processes
in the parent namespace cannot see the target's `/proc` entries. No
`/proc/<pid>`, no `/proc/<pid>/environ`, no way to read the secret.

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

When running with elevated privilege (`geteuid() != getuid()` or
`getegid() != getgid()`), `underwrap-exec` ignores `UNDERWRAP_CONFIG_DIR`
and uses the hardcoded path `/etc/underwrap/wrappers/`. The target path in
the sidecar config must be absolute — relative paths are rejected.

This prevents an attacker from pointing the wrapper at an attacker-controlled
config that redirects the target to a binary that exfiltrates the secret.

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

## Rejected approaches

| Approach | Why rejected |
|----------|-------------|
| Trust client-supplied wrapper names | Trivially spoofable |
| `/proc/<pid>/exe` identity check | All wrappers are the same binary |
| `PR_SET_DUMPABLE` on target | Kernel resets after exec without credential mismatch |
| `suid_dumpable=0` sysctl | Same exec-time reset issue |
| User namespaces | EPERM after setgid credential change |
| Combined setgid + capabilities | NixOS security.wrappers rejects this combination |
| Fallback to direct exec | Silent security degradation |
| Per-wrapper dedicated UIDs | Target can't access user's files |
| `memfd_create` injection | Unnecessary given PID namespace isolation |
| stdin/tempfile injection | Unnecessary given PID namespace isolation |
