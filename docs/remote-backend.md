# Remote backend

Remote backend mode lets you keep secrets on a trusted machine — typically your workstation — while still running wrappers on a different NixOS machine.

The important property is simple:

> the target machine runs the wrappers, but it does not need to store the secrets locally

## When remote mode is useful

Use it when:

- the target machine is ephemeral or lightly trusted
- the wrapper host is an agent VM or automation box
- you want a clean "connect = available, disconnect = unavailable" model
- you want high-value secret material to stay on your workstation

## The model

In remote mode, there are two daemons:

- **`underwrapd`** on the target machine
- **`underwrap-backendd`** on the trusted backend machine

An SSH remote forward exposes the backend socket to the target machine.

```text
target machine                             trusted workstation
--------------                             -------------------
wrapper
  -> underwrapd
       -> /run/underwrap-<instance>/tunnel/backend.sock
             -> SSH remote forward
                  -> underwrap-backendd
                       -> local secret storage
```

From the user's point of view, the wrapper command still looks normal.

## What changes in configuration

On the target machine, you enable remote mode for the instance:

```nix
services.underwrap.instances.work = {
  remote.enable = true;

  credentials."github/gh" = {
    mode = "libsecret";
  };

  wrappers.gh-auth = {
    target = "${pkgs.gh}/bin/gh";
    use."github/gh".access = "read-write";
  };
};
```

The wrapper definitions look the same. The difference is where `underwrapd` reads from.

## Backend daemon setup

On the trusted machine, run `underwrap-backendd` with a backend config.

A minimal example:

```yaml
socket: /run/underwrap-backend/backend.sock
storage:
  backend: plain
  dir: /home/you/.underwrap/secrets
```

Then start:

```bash
underwrap-backendd --config ~/.underwrap/backend.yaml
```

## Establishing the tunnel

The NixOS module creates a per-instance tunnel user such as:

```text
underwrap-work-tunnel
```

From the trusted machine, create the remote forward:

```bash
ssh -N \
  -R /run/underwrap-work/tunnel/backend.sock:/run/underwrap-backend/backend.sock \
  underwrap-work-tunnel@target-machine
```

That is the key path for the generated module layout:

```text
/run/underwrap-work/tunnel/backend.sock
```

## Availability model

Remote mode has a nice operational property:

- tunnel up -> wrappers can read secrets
- tunnel down -> wrappers fail to read secrets

In practice, that means:

- **connecting the tunnel makes secrets available**
- **disconnecting the tunnel removes backend access**

That is not a complete authorization model on its own, but it is a very useful operational boundary.

## Security boundaries

### What remote mode improves

- secrets do not need to be stored on the target machine
- high-value secret material can stay on the trusted workstation
- the target still benefits from underwrap's normal wrapper chain and identity checks

### What remote mode does not change

- the target process still receives the credential material it needs for the invocation
- `root` on the target machine is still `root`
- trust shifts to the backend machine and the SSH tunnel setup

### Backend daemon trust model

`underwrap-backendd` does not implement its own complex multi-tenant authentication layer.

The intended trust boundary is:

- the backend socket is local to the trusted machine
- it stays protected by filesystem permissions
- the only remote exposure path is the SSH forward you explicitly create

In other words: remote mode assumes you trust the workstation that holds the backend daemon.

## Tunnel user notes

The NixOS module creates the tunnel user without a normal shell and places it in the daemon group so the forwarded socket can be created where `underwrapd` expects it.

For production-style deployments, the SSH key used for that tunnel should be restricted.

A typical `authorized_keys` restriction looks like:

```text
restrict,streamlocal-forward=/run/underwrap-work/tunnel/backend.sock ssh-ed25519 AAAA...
```


## Troubleshooting

### Wrapper fails with backend connection errors

Check whether the SSH tunnel is up and whether the forwarded socket exists:

```bash
ls -l /run/underwrap-work/tunnel/
```

### Tunnel user cannot create the socket

Make sure:

- remote mode is enabled for the instance
- the runtime tunnel directory exists
- OpenSSH is configured to allow stream-local forwarding

The module sets the needed OpenSSH server setting for remote-enabled instances.

### Secrets seem to be local anyway

Check your instance configuration. If `remote.enable = true` is not set, the instance will use local storage instead.

## Related docs

- [Configuration reference](configuration.md)
- [Security](security.md)
