# Architecture

## Components

| Binary | Role | Privilege |
|--------|------|-----------|
| `underwrapd` | Frontend daemon: identity verification, unlock state, audit | Runs as `underwrap-<instance>` user |
| `underwrap-backendd` | Backend daemon: secret storage, OAuth derivation | Runs as operator user (your workstation) |
| `underwrapctl` | CLI: store secrets, manage unlock state | Connects to control socket (root/daemon only) |
| `underwrap-exec` | Wrapper stage 1: get secret, pipe to stage 2 | Installed setgid via `security.wrappers` |
| `underwrap-ns` | Wrapper stage 2: PID namespace, exec target | Installed with `cap_sys_admin` |

## Crate structure

```
crates/
  underwrap-protocol/   # Shared types: IPC framing, config schema, messages
  underwrap-store/      # StorageBackend trait + implementations
  underwrapd/           # Frontend daemon
  underwrap-backendd/   # Backend daemon
  underwrapctl/         # CLI tool
  underwrap-exec/       # Wrapper stage 1
  underwrap-ns/         # Wrapper stage 2
  integration-tests/    # Cargo integration tests
```

## Request flow

```
User runs /run/wrappers/bin/default-gws gmail users.labels list ...
  │
  ▼  NixOS security.wrappers sets effective GID
underwrap-exec (setgid)
  ├── connects to /run/underwrap-default/read.sock
  ├── sends WrapperRequest::Read
  │
  ▼  daemon reads SO_PEERCRED, gets effective GID from /proc/<pid>/status
underwrapd (frontend)
  ├── maps egid → wrapper name (kernel-verified identity)
  ├── checks unlock state
  ├── calls backend: store.read() or remote.derive()
  ├── returns material via WrapperResponse::Ok
  │
  ▼  wrapper receives secret
underwrap-exec
  ├── drops privilege (uid, gid, groups)
  ├── prctl(PR_SET_DUMPABLE, 0) — protects own memory
  ├── forks, pipes secret to child on fd 3
  │
  ▼  child execs underwrap-ns
underwrap-ns (cap_sys_admin)
  ├── reads secret from fd 3
  ├── prctl(PR_SET_DUMPABLE, 0) — protects own memory
  ├── unshare(CLONE_NEWPID) — new PID namespace
  ├── drops all capabilities
  ├── forks (child = PID 1 in namespace)
  ├── child: RLIMIT_CORE=0, sets env var, execs target
  │
  ▼  parent zeroizes secret, waitpid, forwards exit code
target (e.g. gh, gws)
  ├── secret in environment variable
  ├── invisible to parent PID namespace
  ├── no core dumps possible
  └── runs normally
```

## IPC protocol

All communication uses length-prefixed JSON frames over Unix domain sockets:

```
[4 bytes big-endian length][JSON payload]
```

Maximum frame size: 64 KiB.

### Read socket (wrapper ↔ daemon)

`WrapperRequest` (tagged by `action`):
- `read` — request secret material
- `unlock` — request unlock (initiates SSH challenge if configured)
- `unlock_response` — complete SSH challenge-response unlock

`WrapperResponse` (tagged by `status`):
- `ok` — success (material and/or message)
- `challenge` — SSH challenge nonce for unlock
- `error` — failure (code + message)

### Control socket (underwrapctl ↔ daemon)

`ControlRequest` (tagged by `command`):
- `status` — query daemon/wrapper status
- `store` — write secret material
- `unlock` — create unlock grant
- `lock` — revoke unlock grant

### Backend socket (frontend ↔ backend)

`BackendRequest` (tagged by `action`):
- `read` — read raw secret
- `derive` — read + apply derivation (e.g. OAuth)
- `write` — store secret
- `status` — health check

`BackendResponse` (tagged by `status`):
- `ok` — success (material and/or info)
- `error` — failure

## Storage backends

The `StorageBackend` trait:

```rust
pub trait StorageBackend: Send + Sync {
    fn read(&self, secret_id: &str) -> Result<Option<String>>;
    fn write(&self, secret_id: &str, material: &str) -> Result<()>;
}
```

Implementations:

| Backend | Config | Location | Use case |
|---------|--------|----------|----------|
| `PlainBackend` | `backend: plain` | Local disk | Single-machine, trusted host |
| `EncryptedBackend` | `backend: encrypted` | Local disk | At-rest encryption (key file) |
| `RemoteBackend` | `backend: remote` | Unix socket | Secrets on remote machine via SSH tunnel |

## OAuth derivation

For wrappers with `derivation: oauth_refresh_to_access_token`, the backend:

1. Reads the stored authorized-user JSON (contains `client_id`, `client_secret`, `refresh_token`)
2. POSTs to the hardcoded Google token endpoint (`https://oauth2.googleapis.com/token`)
3. Returns the short-lived access token (not the refresh token)

The wrapper only ever sees the access token. The refresh token stays in the backend's storage.
