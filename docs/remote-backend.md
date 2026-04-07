# Remote backend

In remote mode, secrets never exist on the target machine. A backend daemon
runs on your workstation (or any trusted machine) and serves secrets over
an SSH-tunneled Unix socket.

The backend daemon (`underwrap-backendd`) is the one component that can run
on any Linux system, not just NixOS. It has no setgid requirements, no
capability requirements, no complex user management — just a binary that
listens on a socket and serves secrets from a directory.

## Architecture

```
Your workstation                          Target machine (NixOS)
─────────────────                         ──────────────────────
underwrap-backendd                        underwrapd (frontend)
  ├── PlainBackend (secrets on disk)        ├── wrapper identity (setgid GID)
  ├── OAuth derivation                      ├── PID namespace isolation
  ├── listens on backend.sock               ├── non-dumpable wrappers
  │                                         ├── connects to backend.sock
  │                                         │
  └── SSH -R ─────────────────────────────► └── /run/underwrap-<inst>/backend.sock
      (you establish this consciously)           (0660, instance group)
```

## How it works

1. You start `underwrap-backendd` on your workstation with your secrets
2. You SSH into the target machine, forwarding the backend socket
3. The frontend daemon on the target connects to the forwarded socket
4. Wrappers on the target read secrets through the frontend → backend chain
5. You disconnect SSH → socket disappears → wrappers fail → locked

**Connecting the tunnel IS the unlock. Disconnecting IS the lock.**

## Setup

### Backend (your workstation)

Create a config file:

```yaml
socket: /run/underwrap-backend/backend.sock
storage:
  backend: plain
  dir: /home/you/.underwrap/secrets
```

Run the backend:

```bash
underwrap-backendd --config ~/.underwrap/backend.yaml
```

Store secrets:

```bash
mkdir -p ~/.underwrap/secrets/github
echo -n 'ghp_...' > ~/.underwrap/secrets/github/token

mkdir -p ~/.underwrap/secrets/google
cat ~/oauth-credentials.json > ~/.underwrap/secrets/google/workspace-creds
```

### Frontend (NixOS target machine)

```nix
services.underwrap.instances.work = {
  remote.enable = true;

  wrappers = {
    gh-authorized = {
      target = "${pkgs.gh}/bin/gh";
      secret = "github/token";
      injection = { mode = "env"; variable = "GH_TOKEN"; };
    };
    gws = {
      target = "${pkgs.gws}/bin/gws";
      secret = "google/workspace-creds";
      injection = { mode = "env"; variable = "GWS_TOKEN"; };
      derivation = "oauth_refresh_to_access_token";
    };
  };

  secrets = {
    "github/token" = {};
    "google/workspace-creds" = { mutable = true; };
  };
};
```

### Establishing the tunnel

From your workstation, forward the backend socket to the target:

```bash
ssh -N \
  -R /run/underwrap-work/backend.sock:/run/underwrap-backend/backend.sock \
  underwrap-work-tunnel@target-machine
```

The `underwrap-work-tunnel` user is created by the NixOS module. It has
no shell and exists only to receive the socket forward.

### Using wrappers

Once the tunnel is up:

```bash
work-gh-authorized pr list          # works
work-gws gmail list         # works, OAuth token derived on your workstation
```

Disconnect the SSH session:

```bash
# Wrappers immediately fail:
work-gh-authorized pr list
# Error: connecting to backend...
```

Reconnect:

```bash
ssh -N -R ... underwrap-work-tunnel@target-machine
# Wrappers work again.
```

## Security properties

- Secrets never touch the target machine's disk
- The backend socket is only accessible to the daemon user (instance group)
- The tunnel user cannot read secrets — it can only forward the socket
- The frontend daemon applies all the same protections: GID identity,
  PID namespace, non-dumpable wrappers, core dump prevention
- OAuth derivation happens on your workstation — the refresh token never
  leaves your machine

## Tunnel user restrictions

The NixOS module creates `underwrap-<instance>-tunnel` with:
- No shell
- Home directory only for `.ssh/authorized_keys`
- Member of the instance's daemon group (so the forwarded socket is
  accessible to the daemon)

For production use, restrict the SSH key in `authorized_keys`:

```
restrict,streamlocal-forward=/run/underwrap-work/backend.sock ssh-ed25519 AAAA...
```

This prevents the tunnel key from doing anything except forwarding that
one specific socket.

## Multiple targets

Run separate tunnel sessions to different machines:

```bash
# Terminal 1: tunnel to dev VM
ssh -N -R /run/underwrap-work/backend.sock:... underwrap-work-tunnel@dev-vm

# Terminal 2: tunnel to staging VM
ssh -N -R /run/underwrap-work/backend.sock:... underwrap-work-tunnel@staging-vm
```

Each target machine has its own underwrap instance with its own tunnel user.
Your single backend serves all of them through separate SSH sessions.
