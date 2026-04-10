# Remote backend

underwrap always uses the same frontend contract:

```text
/run/underwrap-<instance>/backend/backend.sock
```

The frontend daemon connects to that socket whether the backend is local or remote.

The only question is: **who provides that socket?**

- local deployment: a local `underwrap-backendd` export
- remote deployment: an SSH forward to a trusted machine running `underwrap-backendd`

## Components

A remote deployment has two roles:

- **target machine** — runs `underwrapd`, wrappers, and the wrapped tools
- **backend machine** — runs `underwrap-backendd` and stores the secrets

The backend machine also exposes a privileged management socket for backend operators.

## Static backend config

`underwrap-backendd` uses a static config file that defines:

- the management socket
- the predefined stores

Example:

```yaml
management:
  socket: /run/underwrap-backend/mgmt.sock

stores:
  work:
    backend: plain
    dir: /var/lib/underwrap-backend/work
```

Start the backend daemon on the trusted machine:

```bash
underwrap-backendd --config ~/.config/underwrap/backend.yaml
```

## Exporting a store to a data socket

Before forwarding, export the store onto a normal backend protocol socket on the backend machine:

```bash
underwrap-backend-ctl \
  --socket /run/underwrap-backend/mgmt.sock \
  export work /run/underwrap-backend/work.sock --mode 0600
```

That creates a regular backend data socket serving the `work` store.

The frontend still does not know the store name. It only ever sees a socket.

## Forwarding to the target machine

Now satisfy the target machine's expected per-instance socket path with SSH remote forwarding:

```bash
ssh -N \
  -R /run/underwrap-work/backend/backend.sock:/run/underwrap-backend/work.sock \
  underwrap-work-tunnel@target-machine
```

After that, the target machine has:

```text
/run/underwrap-work/backend/backend.sock
```

and `underwrapd` can use the remote backend exactly as if it were local.

## Why the frontend does not name stores

The frontend does **not** send a store name over the protocol.

Instead:

- backend config defines stores
- backend management tooling exports one chosen store to one chosen socket
- socket identity selects the store

That keeps backend routing out of the frontend API and avoids trusting client-supplied namespace or store identifiers.

## Security notes

The management socket is privileged. Treat it as an administrative interface.

Whoever can access it can:

- inspect available stores
- export stores onto sockets
- remove exports

So it should normally be accessible only to a backend operator user or root.

The forwarded per-instance socket on the target machine is much less privileged. It only exposes the already-selected store view that was exported on the backend side.

## Current operator workflow

Today, the intended flow is:

1. configure and start `underwrap-backendd` on the backend machine
2. export a store with `underwrap-backend-ctl`
3. satisfy the target machine's expected socket path locally or via SSH forwarding
4. run the frontend instance normally

Future helper tooling can build on the same model and automate the SSH-forward lifecycle without changing the frontend contract.
