# underwrap

Wrap command execution with just-in-time secret injection and privilege drop.

**NixOS only.** underwrap targets NixOS exclusively. The security model
depends on `security.wrappers` (setgid binaries), declarative user/group
management, file capabilities, systemd service hardening, and tmpfiles —
all orchestrated by a NixOS module. Reproducing this correctly on other
distributions would require extensive manual configuration that is
difficult to get right and harder to maintain. The only component designed
to run on non-NixOS systems is the backend daemon (`underwrap-backendd`),
which may run on any Linux machine holding your secrets.

## What it is

underwrap is a secret brokerage system for local CLI tool execution.
A daemon holds secrets and serves them to wrapper binaries over Unix sockets.
The wrappers inject secrets into the target tool's environment, then exec it
inside an isolated PID namespace where it's invisible to other processes.

```
$ work-gh-authorized pr list
# ↑ This just works. Behind the scenes:
#   1. Setgid wrapper connects to daemon
#   2. Daemon verifies identity from kernel-provided GID
#   3. Daemon fetches secret (local or via SSH tunnel from your workstation)
#   4. Wrapper drops privilege, creates PID namespace
#   5. Target runs with secret in env, invisible to other processes
```

## Why it exists

Every local secret management approach has problems:

- **Environment variables** leak into shell history, `/proc/pid/environ`,
  child processes, agent configs, and service files.
- **Plaintext files** work until you need mutable credentials (OAuth refresh
  tokens) — then you're re-encrypting files, rebuilding systems, or manually
  copying secrets around.
- **Desktop keyrings** are GUI-centric, not designed for headless CLI wrappers.
- **Remote secret managers** (Vault, AWS SSM) add latency, availability
  dependencies, and authentication complexity for local tool invocations.
- **Dotenv / direnv** put secrets in shell scope where any process can read them.

underwrap solves this differently. See [How underwrap compares](docs/comparison.md)
for a detailed analysis.

## Key features

- **Wrapper-first UX**: `work-gh-authorized pr list` feels like running `gh` directly.
  No environment setup, no secret prefixing, no source files.

- **Kernel-verified identity**: each wrapper runs `setgid` to a dedicated group.
  The daemon verifies the effective GID via `SO_PEERCRED` — not a client-supplied
  string. A process can only read the secret mapped to its kernel-assigned GID.

- **PID namespace isolation**: the target runs inside its own PID namespace.
  Same-UID processes cannot see it in `/proc`, cannot read its environment,
  cannot ptrace it. This is enforced by the kernel, not by file permissions.

- **Two-stage wrapper**: stage 1 (setgid) handles identity and secret retrieval.
  Stage 2 (`cap_sys_admin`) creates the PID namespace. Splitting these avoids
  the NixOS `security.wrappers` limitation that setgid and capabilities are
  mutually exclusive.

- **Non-dumpable wrapper processes**: both wrapper parents call
  `prctl(PR_SET_DUMPABLE, 0)` and zeroize the secret after piping it.
  Since they never exec again, the flag persists for their entire lifetime.

- **Core dump prevention**: `RLIMIT_CORE=0` set before exec of the target.
  A crashing tool cannot write the secret to disk.

- **Remote backend**: secrets can live on your workstation, served to the
  target machine over an SSH-tunneled Unix socket. Connecting the tunnel
  makes secrets available. Disconnecting locks them. No secrets on disk on
  the target machine, ever.

- **Multi-instance**: multiple fully isolated instances on one machine.
  Separate users, groups, sockets, directories. No overlap. A compromised
  instance cannot reach another.

- **OAuth derivation**: the daemon (or backend) exchanges OAuth refresh
  tokens for short-lived access tokens. The wrapper only sees the access
  token. The refresh token never leaves the backend.

- **Self-service unlock**: users can unlock wrappers via SSH agent
  challenge-response, without needing root access.

- **Structured audit trail**: every read, write, unlock, lock, and denial
  is logged with wrapper name, secret ID, peer UID, and scope. Secret
  material is never logged.

- **Immutable secrets**: secrets declared `mutable: false` cannot be
  overwritten. The daemon enforces this with per-secret write serialization
  to prevent TOCTOU races.

- **NixOS-native**: the module generates users, groups, setgid wrappers,
  capability binaries, sidecar configs, systemd services, tmpfiles, and
  SSH tunnel users — all from a declarative Nix configuration.

## Quick start

```bash
nix develop
cargo build
cargo test --workspace
nix build .#checks.x86_64-linux.vm-test
```

See [Getting started](docs/getting-started.md) for a full NixOS setup walkthrough.

## Documentation

- [Getting started](docs/getting-started.md) — setup, first wrapper, verification
- [Architecture](docs/architecture.md) — components, request flow, protocols
- [Configuration](docs/configuration.md) — NixOS module, YAML, multi-instance
- [Security](docs/security.md) — threat model, every mechanism, rejected alternatives
- [Remote backend](docs/remote-backend.md) — SSH tunnel setup, lifecycle
- [Comparison](docs/comparison.md) — how underwrap differs from other solutions
- [Testing](docs/testing.md) — cargo tests, VM test assertions
- [TODO](TODO.md) — remaining work

## License

MIT
