# underwrap — TODO

## Next: split-daemon architecture (remote backend over SSH tunnel)

### Goal

Secrets never live on the target VM. A remote backend holds secret material
and is only reachable while an externally-established SSH tunnel is active.
Connecting the tunnel IS the unlock. Disconnecting IS the lock.

### Architecture

```
Your workstation                          Target VM
─────────────────                         ──────────────────────
underwrap-backendd                        underwrapd (frontend)
  ├── StorageBackend (plain/encrypted)      ├── wrapper identity (setgid egid)
  ├── OAuth derivation + token storage      ├── unlock state (= tunnel alive?)
  ├── listens on Unix socket                ├── PID namespace isolation
  │                                         ├── connects to backend.sock
  └── SSH tunnel ─────────────────────────► └── /run/underwrap/backend.sock
      (you initiate this consciously)            (0600 underwrap:underwrap)
```

### Components

**1. underwrap-backendd** (new binary, runs on your workstation)
- Implements the storage + derivation logic currently in underwrapd
- Listens on a Unix socket
- Speaks a simple request/response protocol: `read(secret_id) → material`,
  `write(secret_id, material)`, `derive(secret_id, mode) → derived_material`
- Holds the actual secrets (plain or encrypted backend)
- Does OAuth token exchange and stores the resulting access token
- No wrapper identity logic, no PID namespaces — just a secret server

**2. underwrapd (modified frontend)**
- Keeps: wrapper identity, unlock state, audit, PID namespace, setgid
- Drops: local StorageBackend, local derivation, local token cache
- New: `RemoteBackend` that connects to `/run/underwrap/backend.sock`
- read path: resolve wrapper identity → check unlock → call backend.read()
  over the tunnel socket → return material to wrapper
- Unlock state: tunnel alive = unlocked. Tunnel gone = locked.
  The daemon polls or watches the socket. No explicit unlock/lock commands
  needed (though they could still work for additional time-scoping).

**3. SSH tunnel setup** (NixOS module generates this)
- Dedicated `underwrap-tunnel` system user on the VM
  - No shell, no home, no other access
  - Only purpose: receive the inbound SSH connection
- `authorized_keys` with `restrict,streamlocal-forward=...`:
  ```
  restrict,streamlocal-forward=/run/underwrap/backend.sock ssh-ed25519 AAAA...
  ```
- The socket is created by `sshd` with ownership matching the SSH user
- Daemon user (`underwrap`) needs read access → either:
  - `underwrap-tunnel` user is in the `underwrap` group, socket created 0660
  - Or use `StreamLocalBindUnlink yes` + tmpfiles for ownership

**4. Protocol between frontend and backend**
- Length-prefixed JSON frames (same as existing IPC, reuse `underwrap_protocol::ipc`)
- Messages:
  ```
  BackendRequest:
    Read { secret_id }
    Derive { secret_id, derivation_mode }
    Write { secret_id, material }
    Status { }

  BackendResponse:
    Ok { material? }
    Error { message }
  ```
- No auth on this protocol — the tunnel socket permissions are the auth boundary
- No wrapper identity — the frontend already verified that

### Implementation plan

**Step 1: Backend protocol** (in `underwrap-protocol`)
- Add `BackendRequest` / `BackendResponse` message types
- Reuse existing IPC framing

**Step 2: underwrap-backendd** (new crate)
- Binary that loads config, opens StorageBackend, listens on Unix socket
- Handles Read, Write, Derive, Status
- Derive does the OAuth exchange (move from underwrapd to here)
- Token storage is just Write to the backend's store

**Step 3: RemoteBackend** (in `underwrap-store`)
- Implements `StorageBackend` trait
- Connects to a Unix socket, sends BackendRequest, reads BackendResponse
- Sync (the trait is sync) — blocks on the socket I/O

**Step 4: underwrapd frontend mode**
- Config option: `backend: remote` with `socket: /run/underwrap/backend.sock`
- Uses RemoteBackend instead of PlainBackend/EncryptedBackend
- Derive moves to the backend — frontend just does Read
- Unlock state: check if backend.sock is connectable = unlocked

**Step 5: NixOS module**
- `underwrap-tunnel` user with restricted SSH access
- `services.underwrap.remote.enable = true` mode
- Generates `authorized_keys` for tunnel user
- Adjusts socket permissions for tunnel → daemon access
- Optional: systemd path unit that watches backend.sock existence

**Step 6: VM test**
- Two-node NixOS test: "workstation" and "vm"
- workstation runs underwrap-backendd with secrets stored
- SSH tunnel established from workstation to vm
- vm's underwrapd connects to tunneled backend.sock
- Wrapper on vm reads secret → arrives from workstation
- Tunnel drops → wrapper read fails (locked)
- Tunnel reconnects → wrapper read succeeds again

### What doesn't change

- underwrap-exec: same binary, same two-stage flow, same PID namespace
- underwrap-ns: same binary, same cap_sys_admin
- Wrapper identity: same setgid egid model
- Read socket protocol: same WrapperRequest/WrapperResponse
- Security properties: all local hardening preserved

### What this replaces

- Local storage on the VM (no more PlainBackend/EncryptedBackend on the target)
- Local OAuth derivation on the VM (moves to workstation)
- Explicit unlock/lock commands (tunnel lifetime replaces them)
- The encrypted backend key management problem (key lives on your workstation, not the VM)

## Other future work

- [ ] `underwrapctl reauth` workflow for re-doing OAuth browser flows
- [ ] Secret metadata / versioning
- [ ] sops / age import pipeline
