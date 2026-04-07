# How underwrap compares

This document explains how underwrap differs from existing secret management
approaches and why those approaches are insufficient for the wrapper use case.

## The problem space

underwrap targets a specific scenario: a user runs a CLI tool (like `gh`,
`gws`, or `slack-cli`) that needs a secret (API token, OAuth credential)
injected into its environment. The secret should not be visible to other
processes, should not persist in the user's shell, and ideally should not
exist on the target machine's disk.

Most existing tools solve adjacent problems but not this one.

## Environment variables

**How it works**: `export GH_TOKEN=ghp_...` in shell RC or a wrapper script.

**Problems**:
- The token lives in the shell's environment for the entire session
- Every child process inherits it
- Other same-UID processes can read it from `/proc/<pid>/environ`
- It appears in `/proc/<pid>/cmdline` if passed via `env GH_TOKEN=... gh`
- Shell history may record it
- Process supervisors and agent frameworks can see it

**How underwrap differs**: the secret is injected only into the target
process, which runs in an isolated PID namespace. No other process can see
its `/proc` entries. The wrapper processes that briefly hold the secret are
non-dumpable and zeroize it after use.

## Dotenv / direnv

**How it works**: `.env` files or `direnv` set variables in the shell scope.

**Problems**:
- Same as environment variables — the secret lives in the shell
- `.env` files are plaintext on disk, often accidentally committed to git
- `direnv` provides per-directory scoping but not per-process isolation
- All processes in the shell see all the secrets

**How underwrap differs**: secrets are never in the shell environment.
Each wrapper gets exactly one secret, delivered just-in-time, isolated
by PID namespace.

## SOPS / age

**How it works**: secrets encrypted in git, decrypted at build/deploy time.

**Problems**:
- Decrypted secrets end up as plaintext files on disk
- Mutable credentials (OAuth refresh tokens) don't fit the encrypt-in-git model
- Rotating a secret requires re-encrypting and redeploying
- The decrypted form is available to any process that can read the file

**How underwrap differs**: secrets are served on demand over a Unix socket,
not materialized as files. In remote mode, they never touch the target disk.
Mutable secrets (OAuth tokens) are a first-class concept — the backend
handles token exchange and storage.

## agenix / sops-nix

**How it works**: NixOS modules that decrypt secrets at activation time
into `/run/secrets/`.

**Problems**:
- Decrypted secrets are plaintext files, readable by the configured user/group
- No per-process isolation — any process running as the right user can read them
- Mutable credentials require system rebuild to update
- No concept of unlock/lock or time-scoped access

**How underwrap differs**: secrets are delivered per-process, not per-file.
The daemon mediates access with GID-based identity verification. Mutable
secrets can be updated without rebuilds. Unlock gates add time-scoped access
control. Yama LSM + PID namespace isolation prevents same-UID `/proc` inspection.

## HashiCorp Vault / AWS Secrets Manager / cloud KMS

**How it works**: remote API-based secret retrieval with authentication,
policies, and audit logging.

**Problems**:
- Requires network access and a running remote service
- Adds latency and availability dependency for local CLI tool invocations
- Authentication setup (tokens, IAM roles, AppRole) adds complexity
- The retrieved secret still ends up in an environment variable or file,
  with no process isolation
- Often overkill for a developer workstation wrapping `gh` and `gws`

**How underwrap differs**: underwrap is a local-first system. In local mode,
there's no network dependency. In remote mode, the dependency is a simple
SSH tunnel to your own workstation — not a cloud service. The daemon
handles both storage and process-level isolation. No cloud authentication
infrastructure required.

## 1Password CLI / Bitwarden CLI

**How it works**: `op run -- gh pr list` or `bw get password github-token`.

**Problems**:
- The retrieved secret is placed in the environment of the child process,
  with no isolation from same-UID processes
- Requires the password manager to be unlocked (interactive, may prompt)
- The CLI tools are designed for human use, not for headless/automated
  wrapper invocation
- No setgid identity verification — any process can call `op run`
- No PID namespace isolation

**How underwrap differs**: underwrap is designed for wrapper use, not human
retrieval. Identity is kernel-verified (setgid GID), not trust-the-caller.
The target runs in a PID namespace. Unlock can be time-scoped and
SSH-agent-authenticated without interactive prompts.

## systemd credentials

**How it works**: `LoadCredential=` and `SetCredential=` in systemd units
deliver secrets to services via file descriptors.

**Problems**:
- Designed for service credentials, not for interactive CLI tool wrapping
- The credential is available to the entire service, not per-invocation
- No concept of per-wrapper identity or per-process isolation
- Requires systemd as the process supervisor

**How underwrap differs**: underwrap is for interactive and scripted CLI
tool invocations, not long-running services. Each invocation gets its own
PID namespace. Identity is per-wrapper, not per-service.

## Desktop keyrings (GNOME Keyring, KDE Wallet, libsecret)

**How it works**: D-Bus API for storing and retrieving secrets, typically
unlocked by the login session.

**Problems**:
- GUI-centric: designed for desktop applications, not headless CLI wrappers
- Session-scoped: unlocked for the entire login session, not per-invocation
- D-Bus access is typically user-scoped — any process running as the user
  can query any secret via the same D-Bus session
- No per-process isolation

**How underwrap differs**: underwrap is wrapper-centric, not session-centric.
Each wrapper has its own GID-based identity. Secrets are delivered per-process
with PID namespace isolation. Works in headless environments without D-Bus.

## SSH agent

**How it works**: `ssh-agent` holds private keys in memory, provides signing
operations via a Unix socket.

**Problems**:
- Designed specifically for SSH key operations, not general secret storage
- Cannot store arbitrary tokens or credentials
- The agent socket is user-scoped — any process can request signing
- No process isolation for the consumer

**How underwrap differs**: underwrap uses SSH agent as an *authentication
mechanism* for unlock (challenge-response signing), not as a secret store.
The actual secrets are stored in the daemon's backend. SSH agent provides
the "who are you" proof; underwrap provides the "here's your secret"
delivery with full process isolation.

## pass (Unix password manager)

**How it works**: GPG-encrypted files in `~/.password-store/`, accessed via
`pass show github/token` or `pass -c github/token`.

**Problems**:
- Retrieved secret goes to stdout or clipboard — no process isolation
- When used in a wrapper script (`GH_TOKEN=$(pass show ...) gh`), the secret
  lives in the shell's environment and is visible to same-UID processes
- No per-wrapper identity verification — any process can call `pass`
- GPG agent keeps the passphrase cached for the session, not per-invocation
- No mutable credential support (OAuth tokens require manual re-encryption)

**How underwrap differs**: secrets are delivered directly into an isolated
process, not via stdout or shell substitution. Wrapper identity is
kernel-verified. The target's `/proc` entries are protected by Yama LSM. Mutable
credentials are updated in-place without re-encryption.

## Why NixOS only

underwrap's security model requires precise coordination of:

- **Setgid binaries** with per-wrapper dedicated groups (`security.wrappers`)
- **System user and group creation** for each daemon instance, tunnel user,
  and wrapper identity
- **File capabilities** (`cap_sys_admin` on `underwrap-ns`)
- **Systemd service hardening** (`ProtectSystem`, `NoNewPrivileges`, etc.)
- **Tmpfiles** for runtime directories with correct ownership
- **SSH authorized_keys** with `restrict,streamlocal-forward=` restrictions
- **Sidecar config generation** derived from the same source as the daemon config

On NixOS, all of this is generated declaratively from a single module
definition. Adding a wrapper is one attribute set — the module handles user
creation, group creation, binary installation, setgid permissions,
capability assignment, config generation, and service management.

On a traditional Linux distribution, each of these would be a separate
manual step: `useradd`, `groupadd`, `chmod`, `chown`, `setcap`, writing
config files, writing systemd units, managing `authorized_keys`. Getting
one wrong silently breaks the security model. Keeping it consistent across
updates is a maintenance burden that doesn't justify the effort.

The **only exception** is the backend daemon (`underwrap-backendd`). It's
a simple binary that listens on a Unix socket and serves secrets from a
local directory. It has no setgid requirements, no capability requirements,
no complex user management. It can run on any Linux machine — your
workstation, a CI server, wherever your secrets live. In the remote backend
model, this is the component that stays on your trusted machine while the
NixOS frontends run on target VMs.

## Summary

| Feature | underwrap | env vars | sops-nix | Vault | 1Password CLI | pass | keyring |
|---------|-----------|----------|----------|-------|---------------|------|---------|
| Per-process isolation | ✓ PID namespace | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Kernel-verified identity | ✓ setgid GID | ✗ | ✗ | policy-based | ✗ | ✗ | ✗ |
| No secret on disk | ✓ remote mode | ✗ | ✗ | ✓ | ✓ | ✗ | ✓ |
| Mutable credentials | ✓ | manual | rebuild | ✓ | ✓ | manual | ✓ |
| Headless operation | ✓ | ✓ | ✓ | ✓ | partial | ✓ | ✗ |
| No network dependency | ✓ local mode | ✓ | ✓ | ✗ | ✗ | ✓ | ✓ |
| OAuth derivation | ✓ built-in | ✗ | ✗ | plugin | ✗ | ✗ | ✗ |
| Time-scoped unlock | ✓ | ✗ | ✗ | lease-based | session | GPG cache | session |
| NixOS-native module | ✓ | ✗ | ✓ | ✗ | ✗ | partial | ✗ |
| Wrapper-first UX | ✓ | partial | ✗ | ✗ | ✓ | partial | ✗ |
