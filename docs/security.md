# Security

This document explains the security goals behind underwrap, the mechanisms currently used to pursue them, the assumptions those mechanisms rely on, and the main limitations that still matter.

It is intentionally technical and more detailed than the rest of the documentation.

## Security posture and review status

underwrap is aiming at an awkward problem space: running ordinary local CLIs with secrets, while trying to avoid the usual "just export the token and hope for the best" failure modes.

The project makes strong design choices in pursuit of that goal, but it is still early.

Important framing:

- underwrap already appears useful for reducing **accidental** or **casual** secret exposure in many local and agent-facing workflows
- it is also intentionally designed with a more adversarial caller model in mind
- however, deeper review is still welcome and still needed

So the right way to read this document is:

- these are the current security goals
- these are the current mechanisms
- these are the reasons those mechanisms were chosen
- these are the assumptions and limits you should keep in mind

not:

- this project has been formally verified or independently audited end to end

## What underwrap is trying to protect against

underwrap is primarily trying to reduce or block these classes of problems:

1. **Ambient secret exposure**
   - secrets living in the caller's shell environment
   - secrets leaking into dotfiles, helper scripts, or ad-hoc wrappers
   - secrets becoming easy for unrelated child processes to inherit

2. **Same-user secret theft from nearby processes**
   - another process under the same UID inspecting `/proc`
   - ptrace-style access to short-lived wrapper processes
   - easy reads of wrapper or target state from the host namespace

3. **Wrapper impersonation**
   - a process pretending to be a different wrapper in order to read a secret it should not have

4. **Secret persistence outside the intended backend**
   - temp files
   - sidecar files with secret values
   - local caches that silently bypass the backend model

5. **Cross-instance confusion or leakage**
   - one configured instance reaching another instance's resources

6. **Operational drift toward weaker fallbacks**
   - a security mechanism failing and the system quietly switching to a less safe mode

## What underwrap does not protect against

underwrap does **not** solve these problems:

- **root compromise** — `root` can read memory, files, sockets, and process state
- **kernel compromise** — the design depends on Linux kernel behavior
- **malicious or overpowered target tools** — if the target program itself can reveal the secret, underwrap cannot prevent that
- **unreviewed descendant behavior** — if the target or its helpers execute broad or surprising follow-on behavior, underwrap does not sandbox that for you
- **bad wrapper policy design** — if you wrap a broad CLI, you are still giving the caller broad CLI power
- **all possible agent abuse** — underwrap helps with secret delivery; it does not turn arbitrary local execution into a safe sandbox

That distinction matters. underwrap is a secret-delivery and boundary-tightening system, not a general behavioral sandbox.

This is why the recommended pattern for agent-facing use is still to wrap **your own narrow helper**, not a full general-purpose CLI.

## Threat model summary

A reasonable short summary is:

> underwrap is trying to make "use the secret through this intended command" easier than "obtain the raw secret value" for local CLI workflows, especially on NixOS and especially in agent-facing setups.

That is a meaningful security improvement even when it is not absolute.

## Guarantee under the hardened posture

### Preconditions

The guarantee below applies when all of the following hold:

- The target runs on NixOS with
  `services.underwrap.hardenTargetHost = true` (the default). This
  pins `kernel.yama.ptrace_scope = 2`.
- Every configured swap device is encrypted, or the system has no
  swap. The module asserts this at build time when `hardenTargetHost`
  is enabled.
- Wrappers expose narrow helpers only. A narrow helper, in this
  context, is a program that:
  - does not emit the token on `stdout` or `stderr`,
  - does not persist the token to any path readable by the caller,
  - does not accept attacker-controlled flags or configuration that
    cause it to write the token anywhere the caller can read.
- The mTLS client cert and key, the backend admin token, and the
  admin UI session key retain their default ownership and mode
  (readable only by the daemon user that needs them).

### Guarantee

Under those preconditions, there is no known kernel-mediated path by
which a non-root local attacker — including the caller itself — can
read the raw token value on the target host.

### Residual risk

- **Implementation defects** in the kernel, in the TLS or SSH
  libraries, or in underwrap itself. Absence of a known path is not a
  proof of absence.
- **Root compromise on the target host.** Standard exclusion; `root`
  can read process memory directly.

### Operational trade-off

Pinning `kernel.yama.ptrace_scope = 2` disables unprivileged use of
`strace`, `gdb`, and `perf` against user processes. Deployments that
require these tools for normal operation can lower
`kernel.yama.ptrace_scope` explicitly; the module sets the value with
`mkDefault`, so an overriding entry in
`boot.kernel.sysctl` takes precedence. Lowering the value weakens the
guarantee above to "non-ancestor same-UID processes cannot read the
wrapper or target state; an ancestor process (the caller) can."

---

## Design invariants

These invariants are the backbone of the current design. New features should not quietly violate them.

### 1. All secret material flows through `StorageBackend`

Every byte of secret material — whether it is:

- a plain injected token
- mutable credential state
- libsecret-managed data
- directory-credential file contents
- derived access-token source material

must be read from or written to the daemon's `StorageBackend` path.

That means:

- no ad-hoc JSON files containing secret values
- no temp files holding secret values
- no sidecar caches holding secret values
- no alternate persistence path that bypasses backend policy

Why this matters:

- the backend is where encryption-at-rest decisions live
- the backend is where remote-storage behavior lives
- the backend is where the storage model stays auditable and conceptually centralized

A feature that bypasses `StorageBackend` is not a convenience shortcut; it changes the security model.

### 2. Wrapper identity must come from the kernel, not the caller

When the daemon decides which wrapper a connection represents — and
therefore which secrets it is entitled to — the answer must come from
information the caller cannot choose or forge. Any identity signal
that travels through the connection as application data is derived
from, or chosen by, the caller, so it cannot be trusted for
authorization.

The identity signals underwrap uses are both kernel-maintained and
both tied to the installed wrapper binary:

- **Wrapper socket selection.** Each wrapper has its own Unix socket,
  generated by the NixOS module and owned by a per-wrapper group. The
  socket the connection arrived on determines which wrapper the
  daemon expects; this binding is set at module-build time and cannot
  be changed by a connecting process.
- **Effective GID via `SO_PEERCRED`.** The daemon asks the kernel for
  the peer's credentials on the already-established socket. It then
  reads the peer's effective GID from `/proc/<pid>/status` and checks
  that it matches the group the wrapper socket is bound to. The
  effective GID is set by the kernel when a setgid binary execs; a
  process that was not launched through the installed wrapper binary
  cannot have it.

A request is accepted only when both signals agree. Nothing the
caller sends on the connection is used to select a wrapper or to
gate access to secrets, so the following attack classes are ruled
out by construction:

- claiming to be a different wrapper by sending that wrapper's name
  in the request body,
- invoking the wrapper binary via a symlink with a different
  `argv[0]` (the daemon never consults `argv[0]`),
- connecting to a wrapper socket from a process that is not setgid to
  the matching group (the egid check rejects it before any request
  is read).

Concretely, this rules out the most obvious confused-deputy attacks
against the socket layer. A same-UID attacker who does not go through
the installed wrapper binary cannot cause the daemon to treat them as
any wrapper at all.

### 3. Channels carrying secrets must be authenticated at creation time

Any channel that carries secret material or long-lived authenticated capability must be born from an already authenticated context.

In practice this means:

- the wrapper-socket request is authenticated by wrapper identity
- the exec→namespace handoff uses an inherited anonymous pipe
- the libsecret channel is created during an authenticated wrapper request and then inherited forward
- directory credential channels are created during an authenticated wrapper request and then inherited forward

No user-controlled token or environment-variable based side channel is trusted as the primary authorization mechanism.

### 4. File descriptors carrying secret material must not leak across trust boundaries

The current design relies heavily on anonymous, inherited file descriptors. That is good for avoiding filesystem-visible artifacts, but it only works if those fds are closed before the wrong process can inherit them.

In particular:

- the fd 3 pipe is consumed before the target is exec'd
- the libsecret channel fd must not be inherited by the final target process
- directory credential channel fds must not be inherited by the final target process

### 5. Processes that hold secret material should be non-dumpable whenever possible

If a process is holding secret material in memory and does not need to exec a normal target binary afterward, it should call:

```text
prctl(PR_SET_DUMPABLE, 0)
```

This is central to protecting the wrapper processes themselves.

### 6. No silent fallback to a weaker mode

If a security mechanism fails, the wrapper should abort rather than continue in a degraded mode.

This principle matters most in:

- namespace setup
- mount isolation for libsecret mode
- privilege dropping
- descriptor handling

---

## Wrapper identity: setgid + effective GID

### The problem

A caller-controlled wrapper name is not trustworthy. The daemon must be very strict about **who** it believes it is serving.

### The current design

Each wrapper binary is installed setgid to a dedicated Unix group such as:

```text
underwrap-work-jira
```

When the wrapper connects to the daemon:

- it connects to that wrapper's dedicated wrapper socket
- the daemon obtains the peer credentials from the Unix socket
- it reads the peer's effective GID from `/proc/<pid>/status`
- it verifies that GID matches the wrapper expected for that socket

The protocol-level wrapper name is not the authority. The wrapper socket selects the expected wrapper, and the kernel-visible effective GID must confirm it.

### Why effective GID instead of real GID?

`SO_PEERCRED` gives you the real GID, not the effective GID. After a setgid transition, those are not the same thing.

So the daemon uses the peer PID from `SO_PEERCRED` and then reads the effective GID from the `Gid:` line in `/proc/<pid>/status`.

### Why not trust `argv[0]`?

Because `argv[0]` is caller-controlled.

A user can make a symlink with a misleading name and invoke the same underlying wrapper binary through that name. The kernel-assigned effective GID remains authoritative; `argv[0]` does not.

### Why per-wrapper sockets?

Per-wrapper sockets tighten the local trust boundary in a useful way.

They let the filesystem permission model reject unrelated local processes before the daemon spends work on them, while still keeping wrapper identity anchored in the combination of wrapper-socket selection and the wrapper's effective GID.

### Why this is NixOS-shaped

The design leans on `security.wrappers` to install setgid wrapper entry points correctly and consistently. That is one major reason the project is explicitly NixOS-first.

---

## Two-stage wrapper design

### Why split the wrapper?

underwrap needs two things from its wrapper chain:

1. a setgid identity hop
2. the ability to create a PID namespace

On NixOS, those concerns are easier to express as two stages:

- **`underwrap-exec`**: setgid entry wrapper
- **`underwrap-ns`**: capability-bearing namespace helper

### Stage 1: `underwrap-exec`

Responsibilities:

- determine wrapper identity from effective GID
- load the matching sidecar config
- connect to the daemon
- receive secret material
- drop elevated privilege
- hand the secret to stage 2 over fd 3

### Stage 2: `underwrap-ns`

Responsibilities:

- read the secret from fd 3
- create the namespace environment
- set up the target process environment
- apply final process hardening steps
- exec the target

### Why fd 3?

The secret is passed through an anonymous pipe on file descriptor 3.

This is intentionally not:

- an environment variable between wrapper stages
- a command-line argument
- a temp file

Using fd 3 keeps stdin/stdout/stderr untouched and avoids a filesystem-visible or process-list-visible handoff path.

---

## Privilege drop

`underwrap-exec` drops elevated privilege before the target is reached.

The key steps are:

1. `setgroups(0, NULL)`
2. `setresgid(real_gid, real_gid, real_gid)`
3. `setresuid(real_uid, real_uid, real_uid)`

Using the `setres*` calls makes the permanent drop explicit for the real,
effective, and saved IDs.

That matters because underwrap wants a **one-way** drop here, not a temporary
one. Clearing the saved IDs as part of the drop means the wrapper cannot later
regain the elevated setgid-derived identity through saved credentials.

Each return value is checked. A failed privilege drop is a hard failure.

This follows the project's general rule that silently continuing in a weaker or riskier state is worse than aborting.

---

## Protecting the wrapper processes themselves

The wrapper parent processes are among the most sensitive moments in the whole flow. They briefly hold raw secret material in memory before the target is launched.

### The current mechanism

Both `underwrap-exec` and `underwrap-ns` call:

```text
prctl(PR_SET_DUMPABLE, 0)
```

once they hold secret material.

Because those parent processes do not exec another normal target afterward, the dumpable flag remains in effect for the rest of their lifetime.

### Why this matters

For same-UID processes without `CAP_SYS_PTRACE`, setting a process non-dumpable blocks important inspection paths such as:

- `/proc/<pid>/mem`
- `/proc/<pid>/environ`
- ptrace attach
- related `/proc` access checks that rely on `ptrace_may_access()`

### Zeroization

The wrapper processes also zeroize secret buffers after handoff where practical.

That does not make memory disclosure impossible in every imaginable scenario, but it reduces the lifetime of secret material in process memory.

---

## Protecting the final target process

This is the part most likely to be misunderstood.

### The awkward fact

A normal target binary does **not** stay non-dumpable just because the parent set `PR_SET_DUMPABLE=0` before exec.

For an ordinary exec without a continuing credential mismatch, Linux resets the dumpable state.

So the final target process needs protection from a different combination of mechanisms.

### What currently helps protect the target

Three mechanisms together cover the same-UID inspection surface.

#### 1. Yama LSM, pinned to `ptrace_scope=2`

`services.underwrap.hardenTargetHost` defaults to `true`. The module
sets `kernel.yama.ptrace_scope=2` on the target host. At that level,
`ptrace_may_access()` rejects every tracer that does not hold
`CAP_SYS_PTRACE`, which in turn blocks every same-UID access path
that goes through that check:

- `/proc/<pid>/environ`
- `/proc/<pid>/mem`
- `/proc/<pid>/fd/*` content
- `ptrace(PTRACE_ATTACH)`, `strace`, `gdb`, `perf`

Under `scope=2` the check does not distinguish between an ancestor of
the target and an unrelated same-UID process. Neither holds
`CAP_SYS_PTRACE` and neither passes. This is the difference from the
distribution default `scope=1`, where ancestors are exempted and a
caller process can therefore read `/proc/<pid>/environ` on its own
wrapper descendants.

Unprivileged use of `strace`, `gdb`, and `perf` against user
processes is disabled by `scope=2`. Deployments that require these
tools can lower the value explicitly; the sysctl is declared with
`mkDefault`, so an overriding entry takes precedence. With
`scope=1`, the protection narrows to non-ancestor same-UID processes;
a caller process remains able to read its descendant target's
environment.

#### 2. Credential transitions in the wrapper chain

The target descends from a process tree that included:

- an exec of a setgid binary (`underwrap-exec`)
- an exec of a capability-bearing binary (`underwrap-ns`)

Those privilege-affecting exec transitions matter for ptrace
eligibility and help prevent straightforward tracing from the
original ancestor process even when scope is lowered.

`underwrap-ns` also carries `cap_setpcap` and prunes the capability
bounding set before the target execs. It sets `PR_SET_NO_NEW_PRIVS=1`
before the final target exec so the target and its descendants cannot
gain privilege later through setuid/setgid binaries or file
capabilities. The target should therefore run with zero effective,
permitted, inheritable, and ambient capabilities and cannot acquire
new ones on later execs even if the kernel refuses bounding-set
pruning.

#### 3. Caller-controlled environment is cleared

The target is launched with `env_clear()` and only the keys allowed by
the wrapper config are reintroduced. The NixOS module additionally
asserts at build time that no wrapper's `envPassthrough` or `envSet`
contains an `LD_*` variable, so an operator cannot accidentally
re-introduce dynamic-linker redirection.

### What the PID namespace contributes

The PID namespace is still useful, but it is **defense in depth**, not
the sole protection for target secrecy.

It helps by:

- giving the target a separate PID space
- making process discovery less direct from inside the namespace
- combining well with a fresh `/proc` mount in libsecret mode

### Important caveats

The secret sits in the target's environ, in cleartext, for the
target's lifetime. Env-injection requires this. The guarantee stated
in [Guarantee under the hardened posture](#guarantee-under-the-hardened-posture)
therefore rests on three preconditions:

1. **`ptrace_scope=2`.** Pinned by the module.
2. **Encrypted or absent swap.** Asserted by the module when
   `hardenTargetHost = true` so that a token paged out of the
   target's memory cannot surface as plaintext on disk.
3. **Narrow wrappers.** The wrapper author is responsible for
   ensuring the target does not print, log, persist, or otherwise
   exfiltrate the token. Where that responsibility cannot be
   discharged for a given target tool, use libsecret or
   directory-credential mode so the raw value never enters the
   target's environ.

Loosening any of these preconditions narrows the guarantee
correspondingly.

---

## Why not user namespaces?

A natural question is: why not use unprivileged user namespaces and avoid `cap_sys_admin`?

The main issue is that the current identity model depends on a setgid transition, and that does not compose cleanly with the required `uid_map` writes for this design.

In other words, the setgid-based identity mechanism and the most obvious user-namespace approach pull in different directions.

---

## Core dump prevention

Before the final target execs, underwrap sets:

```text
RLIMIT_CORE = 0
```

This is meant to prevent a crashing target from leaving secret-bearing core files behind.

This is a simple mechanism, but it matters. Secret-handling systems should be opinionated about core dumps.

---

## Configuration trust and fixed target paths

`underwrap-exec` does not let the caller choose an arbitrary config location.

It reads sidecar configs from the fixed directory:

```text
/etc/underwrap/wrappers/
```

and the main target path inside that config must be absolute.

This is important because it prevents a caller from trivially redirecting the wrapper to a malicious alternate target binary through environment or path tricks.

The fixed target path is especially important for agent-facing setups, where the distinction between:

- "run this approved helper"
- and
- "run a caller-chosen binary with my secret"

is the entire point.

---

## Environment sanitization

underwrap starts the target from a cleared environment and then reconstructs only a limited, trusted set of environment variables.

### Why this matters

A full inherited caller environment is too powerful.

It enables or complicates things like:

- `LD_PRELOAD`
- `LD_LIBRARY_PATH`
- user-supplied `UNDERWRAP_*` control variables
- arbitrary ambient behavior changes in the target

### The model

- a safe default whitelist is retained
- `PATH` is intentionally **not** inherited from the caller
- if a wrapper needs `PATH`, it must be set explicitly from trusted configuration
- helper-selection variables such as `EDITOR`, `VISUAL`, `PAGER`, and `BROWSER` should also be treated as risky and preferably set from trusted configuration rather than caller passthrough
- extra passthrough variables may be allowed per wrapper
- fixed variables may be set per wrapper from trusted configuration

This is one of the least glamorous but most important parts of the design. A lot of real-world abuse happens through ambient environment control.

---

## Socket permissions and local boundaries

The generated sockets intentionally have different roles.

### Wrapper sockets

Each wrapper gets its own wrapper socket in the instance runtime directory.

These sockets are not world-connectable. They are mode `0660` and grouped to the wrapper's dedicated Unix group.

That means local code must first get into the correct wrapper group context before it can even connect. The daemon then performs a second check by verifying the peer's effective GID for that wrapper socket before reading the first frame.

This is a deliberate choice:

- kernel-level permission gating before connection
- strict daemon-side authorization after connection

### Backend-managed administration

The admin surface is not a Unix socket on the target host. Operators
manage agent grants and the global write-lock through the central
`underwrap-backendd`'s admin HTTP API (bearer-token or session-cookie
authenticated) or through the browser-based admin UI in front of it.
The target host only needs outbound mTLS access to the backend.

---

## Mutation gating

The ticket-245 rewrite replaced the per-secret unlock-until model
with a single global toggle per store, `writes_enabled`. Default:
`false`. Every write path in the admin API
(`/v1/admin/secret/{write,delete}`) consults the flag; when it is
off, the request rejects with `403 writes_globally_disabled` and
nothing reaches storage.

Wrapper-side writes through the libsecret channel inherit the same
gate: when `writes_enabled = false`, the libsecret slot is projected
to the target with `AccessMode::ReadOnly` and the wrapper-side
secretsd refuses store operations against it.

### Operator workflow

To edit a credential, an operator flips the per-store toggle on,
submits the new value, and flips the toggle back off. There is no
auto-disable timer; the manual flow is intentionally explicit. A
prominent banner in the admin UI marks the window so an operator
doesn't leave writes enabled by accident.

### Trust model implication

Mutation immutability is an **operator-discipline property**, not a
schema invariant. While `writes_enabled` is `true`, any agent grant
on the store can drive a write. If a future requirement calls for
true tamper-evidence ("this value was never overwritten"), it has to
come from a different mechanism — append-only storage, audit
attestations — not from the policy layer, since policy itself is
operator-mutable.

### Reserved keys

`/v1/admin/secret/write` refuses outright to write to the reserved
policy-document secret id. Policy edits go through
`/v1/admin/policy/put` (and the dedicated
`/v1/admin/policy/writes-enabled/set`), which are authenticated by
the admin session but are **not** themselves gated by `writes_enabled`
— otherwise the operator could lock themselves out of the toggle.

---

## Audit trail

The daemon emits structured audit events for security-relevant operations such as:

- granted and denied reads
- stores and denied stores
- per-call approval verifications
- authentication denials at the socket layer
- libsecret channel reads, writes, and deletes

The design goal is:

- log the action and relevant identity context
- never log secret material

Logging does not make a system secure, but in systems that are trying to be security-aware, absence of usable audit events is a real weakness.

---

## Central backend security model

Running `underwrap-backendd` centrally changes **where** secrets live,
but not the basic wrapper flow on the target host.

### What the central backend is trying to buy you

- The target host does not need to store any secret material on disk.
- High-value secret material stays on a more trusted workstation or
  server. A compromised target host gets the secret it needed for one
  invocation, not the store.
- Central policy: wrapper modes, rules, and grants are configured in
  one place and enforced by one process. Operators can revoke access
  centrally without touching the target host.

### What it does not buy you

- The target process still receives the credential material it needs
  for the invocation.
- `root` on the target or backend host is still `root`.
- A compromised backend host is still a compromised backend host.

### Backend trust model

`underwrap-backendd` authenticates two distinct surfaces:

- **Runtime** (`/v1/*`) — mutual TLS. Every `underwrapd` instance gets
  one client certificate. The certificate's subject CN is the instance
  principal; the backend maps it to one store and serves only that
  store. Unknown principals are rejected before any request handler
  runs.
- **Admin** (`/v1/admin/*`) — bearer tokens. Each operator has one
  opaque token loaded from a file at startup. Tokens are compared in
  constant time. The matched operator name is recorded with every
  audit event.

The admin listener is plain HTTP. When it needs to be reachable beyond
localhost, the operator is expected to front it with a local
TLS-terminating reverse proxy. The bundled `underwrap-admin-ui` is
itself one such client, designed to sit on a jump host or behind the
same TLS front.

---

## Instance isolation

Each named instance gets isolated:

- daemon user and group
- wrapper groups
- runtime directory
- state directory
- service instance
- optional tunnel user

The goal is to make separate instances structurally separate by default rather than relying only on policy checks inside one giant shared namespace.

---

## Encrypted backend key handling

For the encrypted backend, the master key is handled with zeroization in mind.

This does not magically solve every memory-disclosure scenario, but it reflects the same general design principle used elsewhere in underwrap: sensitive material should not live in memory longer than necessary.

---

## Libsecret mode: the hard part

Libsecret mode is where underwrap's design gets most intricate.

### The problem

A tool such as `gh` may expect a working Secret Service provider reachable through D-Bus.

If it talks to the host's real session bus and real keyring, secret state escapes underwrap's storage model.

So underwrap needs to provide:

- a private bus
- a private Secret Service implementation
- a way for that implementation to reach the backend without authenticating as a normal wrapper again

### The current solution in one sentence

During the authenticated wrapper request, the daemon creates a scoped socketpair and passes one end into the wrapper namespace for `underwrap-secretsd`.

That channel then becomes the long-lived authenticated path between `underwrap-secretsd` and the daemon.

### Why create the channel during the wrapper request?

Because `underwrap-secretsd` itself runs as the ordinary user inside the namespace. It does not have the wrapper's special setgid identity and should not pretend to.

So the right time to authenticate is **before** entering that phase, while the original wrapper request is still authenticated.

### Why this is reasonably strong

The channel is:

- **anonymous** — created by `socketpair()`, not by a discoverable filesystem socket path
- **scoped** — the daemon side binds it to one wrapper's libsecret namespace
- **lifetime-bound** — when the namespace dies, the channel dies

### Daemon-side scoping

The daemon handler for this channel only allows operations inside the wrapper's own libsecret prefix.

That means a compromised `underwrap-secretsd` for one wrapper should not be able to ask the daemon for arbitrary unrelated secrets.

### Descriptor hygiene

A critical rule here is that the final target process must **not** inherit this channel fd.

Only `underwrap-secretsd` should keep it.

If that descriptor leaked to the target, the target would gain a much more direct authenticated path back to the daemon than intended.

---

## Libsecret mode: private D-Bus and mount isolation

A private Secret Service implementation only helps if the target actually talks to that private service.

### The approach

In libsecret mode, `underwrap-ns` launches:

- a private `dbus-daemon`
- `underwrap-secretsd`
- the target process with `DBUS_SESSION_BUS_ADDRESS` pointing at that private bus

### Why mount isolation matters

The setup also uses a mount namespace so that the private bus socket, proc view, and any namespace-private staged filesystem credentials live in a namespace-private filesystem context.

Important steps include:

1. making mounts private
2. mounting a fresh `/proc` inside the PID namespace for libsecret mode
3. mounting private tmpfs roots for namespace-private runtime state such as the D-Bus socket and staged directory credentials
4. using a `noswap` tmpfs for staged directory credentials so those staged files do not page out to swap when the kernel supports the mount option
5. hiding `/run/user/<uid>/bus` if it exists

These steps are not decorative. They are part of keeping the host's normal session bus — and the host's normal home-directory view — from becoming the path of least resistance.

For directory credentials, the same mount-namespace isolation is what keeps backend-managed files out of the host's ordinary home-directory view. Another same-UID host process should not be able to read those mounted files directly from the host namespace while the wrapper runs.

The implementation also treats managed directory trees strictly: it persists only regular files, reconstructs empty directories as needed, and rejects symlinks created inside managed trees during sync.

Like the rest of underwrap's security story, this still depends on kernel behavior. If a deployment kernel does not support the tmpfs options underwrap relies on for directory staging, wrapper execution aborts rather than silently falling back to a weaker mode.

### A real limitation: abstract D-Bus sockets

Filesystem hiding works for filesystem socket paths. It does not block abstract Unix sockets, which live in a different namespace model.

That means there is still a theoretical defense-in-depth gap if some real host session bus were reachable through an abstract socket and a client deliberately ignored the provided `DBUS_SESSION_BUS_ADDRESS`.

In practice, the normal modern session-bus setup on NixOS is path-based and the intended clients are expected to use the provided address. Still, the limitation is worth naming explicitly.

---

## Approval and policy gating model

Whether a wrapper can serve a request is decided by the backend-owned
**policy document**. Operators edit it through the admin HTTP API or
the admin UI; wrappers never see or control it. The document lives
under a reserved key in the same `StorageBackend` that holds the
wrapper's secrets, so policy and secret material share one trust
model.

After ticket 245 the document is a flat list of **agent grants**
keyed by `(wrapper, host, user)` plus the per-store
`writes_enabled` toggle. Each grant carries one of four modes:

- **`Locked`** — every request through this grant is denied.
- **`Unlocked`** — requests are allowed.
- **`UnlockedUntil { until_epoch }`** — requests are allowed until
  the deadline. The deadline is checked lazily on each request;
  there is no backend sweeper.
- **`PerCallApproval { approval_key }`** — each individual request
  must be accompanied by a fresh ssh-agent signature. A prior
  success does not carry over to the next call.

If no grant exists for `(wrapper, host, user)` the request is
rejected with the same "ungranted" shape as a `Locked` grant — the
daemon does not distinguish "no grant" from "denied grant" to the
wrapper, so operators can't be probed for grant existence through
the wrapper channel.

### Per-call ssh-agent approval

When a grant is in `PerCallApproval`, the flow is:

1. The daemon issues a challenge: a random 32-byte nonce and a
   short-lived challenge ID, valid for 60 seconds. The
   `approval_key` bound to the grant (an OpenSSH public key) is
   returned so the caller knows which key to sign with.
2. `underwrap-exec` asks the caller's SSH agent to sign the nonce
   with the matching private key.
3. The daemon verifies the returned signature against the stored
   `approval_key`. Ed25519 and RSA keys are accepted; other SSH key
   types are rejected. The challenge is consumed whether or not the
   signature verifies — each nonce is single-use.

The only thing the caller needs to hold is access to the SSH agent
loaded with the approved key. The private key never touches the
daemon or the wrapper.

### What's no longer in the model

The pre-245 model carried a per-`(wrapper, resource, operation)`
allow/deny ACL on top of the mode gate. That layer is gone. Once
the grant mode says "allow", the daemon serves whatever resources
the wrapper's declaration lists — no per-resource filter. To revoke
an agent's access to a resource, edit the wrapper definition; to
revoke the agent entirely, delete the grant.

---

## Dependency advisory note

The project currently carries `RUSTSEC-2023-0071` through the `rsa` crate used by `ssh-key`.

The relevant context is:

- the advisory concerns RSA decryption timing behavior
- underwrap uses the dependency chain for SSH signature verification during unlock flows
- the daemon does not hold RSA private keys and does not perform RSA decryption

So this is currently understood as a dependency-tree advisory rather than an exploitable path in the intended underwrap code path.

That said, dependency advisories should not be waved away casually; they should be tracked and revisited.

---

## Operational assumptions you should keep in mind

The current design assumes, among other things:

- NixOS-style wrapper installation and service management
- Linux kernel behavior matching the documented mechanisms relied on here
- Yama protections present at the expected setting for the strongest host-side same-UID protection story
- trusted root-owned wrapper configuration on the target host
- sane filesystem permissions on the backend socket in remote mode

If those assumptions change, some of the security conclusions change with them.

---

## Rejected approaches

These ideas were considered and rejected for the current design.

| Approach | Why rejected |
|---|---|
| Trust client-supplied wrapper names | Trivially spoofable |
| Trust `argv[0]` for wrapper identity | User-controlled and vulnerable to symlink confusion |
| Identify wrappers by `/proc/<pid>/exe` | All wrapper entry points are fundamentally the same binary shape |
| Put secrets in argv or env between wrapper stages | Too visible in process state |
| Use temp files for handoff | Unnecessary persistence and filesystem exposure |
| Combine setgid and namespace capability in one wrapper stage | Poor fit for the NixOS wrapper model used here |
| Rely on `PR_SET_DUMPABLE=0` alone for the final target | Exec resets the relevant state for ordinary targets |
| Fall back to weaker execution when isolation fails | Silent security degradation |
| Per-wrapper dedicated UIDs for target execution | Breaks ordinary access to the invoking user's working context |
| User-namespace-based redesign | In tension with the current setgid-based identity model |

---

## Final perspective

underwrap is trying to be honest about a difficult reality:

- local CLI execution is messy
- process state is observable in many places
- "just use an env var" is often more ambient than people realize
- agent-facing workflows make that problem more urgent

The current design goes quite far to reduce those problems on NixOS. That is the point of the project.

But the right security attitude is still:

- appreciate the mechanisms
- understand the assumptions
- design narrow wrappers carefully
- invite review instead of assuming the problem is solved forever

## Related docs

- [Architecture](architecture.md)
- [Agent-facing wrappers](agents.md)
- [Remote backend](remote-backend.md)
- [GitHub via libsecret](gh-libsecret.md)
