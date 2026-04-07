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
inside a PID namespace with Yama LSM protection against same-UID `/proc` inspection.

```
$ work-gh-authorized pr list
# ↑ This just works. Behind the scenes:
#   1. Setgid wrapper connects to daemon
#   2. Daemon verifies identity from kernel-provided GID
#   3. Daemon fetches secret (local or via SSH tunnel from your workstation)
#   4. Wrapper drops privilege, creates PID namespace
#   5. Target runs with secret in env, protected by Yama LSM + PID namespace
```

## Primary use case: agent-facing wrappers without handing agents reusable tokens

A major use case is giving an AI agent or automation system access to a CLI
capability **without** giving it the raw API token.

Typical setup:

- the agent can already read the workspace and invoke approved commands
- you expose a **narrow wrapper** like `agent-gh-comment` or `agent-slack-post`
- that wrapper is deployed declaratively by NixOS and points to a fixed target
  in the Nix store
- underwrap injects the token just-in-time for that wrapper's target

Example:

```
$ agent-gh-comment 123 /workspace/review.md
# The agent can already read /workspace/review.md.
# What underwrap avoids is handing the agent GH_TOKEN itself.
```

This is where underwrap fits well:

- the raw token is not in the agent's shell environment, prompt, or config files
- the configured target path is absolute and comes from a trusted root-owned
  sidecar config — no PATH lookup for the main target
- the target starts from a **clean environment**, blocking `LD_PRELOAD`,
  `LD_LIBRARY_PATH`, and injection of internal `UNDERWRAP_*` variables
- wrapper identity is kernel-verified via setgid effective GID
- wrapper processes are non-dumpable and zeroize secret material after handoff
- core dumps are disabled for the target
- secrets can stay on your workstation entirely in remote-backend mode

Combined with **specialized wrappers** that limit repo / subcommand / flags /
remote action surface, this gives a reasonable defense against raw token
exfiltration and reduces tooling abuse.

It is **not** a full sandbox. If you wrap a broad tool like `gh` directly, you
are still giving the caller broad `gh` capabilities. underwrap is the hardened
secret-delivery layer around those wrappers, not a replacement for wrapper
policy design.

See [Agent-facing wrappers](docs/agents.md) for the recommended pattern.

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

- **PID namespace + Yama protection**: the target runs inside its own PID namespace.
  Same-UID processes cannot read its `/proc/<pid>/environ` or ptrace it,
  enforced by Yama LSM (`ptrace_scope=1`, the NixOS default). The PID
  namespace provides defense-in-depth (different PID space, harder discovery).

- **Two-stage wrapper**: stage 1 (setgid) handles identity and secret retrieval.
  Stage 2 (`cap_sys_admin`) creates the PID namespace. Splitting these avoids
  the NixOS `security.wrappers` limitation that setgid and capabilities are
  mutually exclusive.

- **Non-dumpable wrapper processes**: both wrapper parents call
  `prctl(PR_SET_DUMPABLE, 0)` and zeroize the secret after piping it.
  Since they never exec again, the flag persists for their entire lifetime.

- **Clean target environment**: the target starts from `env_clear()` and only
  receives a safe default whitelist plus trusted per-wrapper additions from the
  root-owned sidecar config. This blocks `LD_PRELOAD` / `LD_LIBRARY_PATH`
  injection and user-controlled `UNDERWRAP_*` overrides.

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

## When to use underwrap

Use underwrap when you want one or more of these:

- **Agent-facing wrappers**: let an agent use a CLI capability without giving
  it the reusable token directly
- **Existing CLI compatibility**: keep using tools like `gh` or other token-
  driven CLIs instead of rewriting everything into a broker service
- **Secrets off the target machine**: keep credentials on your workstation and
  forward them over an SSH-tunneled Unix socket
- **Mutable credentials**: support OAuth refresh/access token workflows or
  libsecret-backed tools
- **NixOS-native deployment**: generate users, groups, setgid wrappers,
  sidecar configs, and services from one declarative config

You probably do **not** need underwrap if:

- plain environment variables or `op run` / `pass` are already good enough
- you control the full integration and can build a small broker service instead
- you need strong behavior restriction more than secret-delivery hardening
- you only have a handful of static secrets and no wrapper-oriented workflow

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
- [Agent-facing wrappers](docs/agents.md) — recommended pattern for AI agents and narrow wrappers
- [Architecture](docs/architecture.md) — components, request flow, protocols
- [Configuration](docs/configuration.md) — NixOS module, YAML, multi-instance
- [Security](docs/security.md) — threat model, every mechanism, rejected alternatives
- [Remote backend](docs/remote-backend.md) — SSH tunnel setup, lifecycle
- [Comparison](docs/comparison.md) — how underwrap differs from other solutions
- [Testing](docs/testing.md) — cargo tests, VM test assertions
- [TODO](TODO.md) — remaining work

## License

MIT
