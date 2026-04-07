# Getting started

This guide walks you through setting up underwrap on NixOS, creating your
first wrapper, and verifying it works.

## Prerequisites

- **NixOS** — underwrap targets NixOS exclusively (see [comparison](comparison.md)
  for why)
- The underwrap flake added to your system configuration

## Adding underwrap to your system

In your flake.nix inputs:

```nix
inputs.underwrap.url = "github:your-org/underwrap  # replace with your actual repo";
```

In your NixOS configuration:

```nix
{ underwrap, pkgs, ... }:

{
  imports = [ underwrap.nixosModules.default ];

  services.underwrap = {
    enable = true;
    package = underwrap.packages.x86_64-linux.default;

    instances.default = {
      wrappers.gh-authorized = {
        target = "${pkgs.gh}/bin/gh";
        secret = "github/token";
        injection = { mode = "env"; variable = "GH_TOKEN"; };
      };

      secrets."github/token" = {};
    };
  };
}
```

Rebuild your system: `sudo nixos-rebuild switch`

## What the module creates

After rebuild, you'll have:

- **System user** `underwrap-default` running the daemon
- **Wrapper group** `underwrap-default-gh-authorized`
- **Setgid wrapper** at `/run/wrappers/bin/default-gh-authorized`
- **PID namespace helper** at `/run/wrappers/bin/underwrap-ns`
- **Daemon service** `underwrapd-default.service`
- **Sidecar config** at `/etc/underwrap/wrappers/default-gh-authorized.yaml`
- **Runtime directory** at `/run/underwrap-default/`

## Storing a secret

Store the GitHub token (as root, since the control socket is restricted):

```bash
echo -n 'ghp_your_actual_token_here' | \
  sudo underwrapctl --socket /run/underwrap-default/control.sock \
  store github/token
```

The secret is stored in `/var/lib/underwrap-default/secrets/github/token`.

## Using the wrapper

Now any user can run:

```bash
default-gh-authorized pr list
```

This is equivalent to `GH_TOKEN=ghp_... gh pr list`, except:

- The token never appears in your shell environment
- The `gh` process runs inside a PID namespace, its `/proc` entries protected by Yama LSM
- The wrapper's memory is non-dumpable (same-UID processes can't read it)
- Core dumps are disabled for the target process

## Verifying it works

Check the daemon is running:

```bash
systemctl status underwrapd-default.service
```

Check the wrapper exists and has the right permissions:

```bash
ls -la /run/wrappers/bin/default-gh-authorized
# Should show setgid bit (s) and the wrapper group
```

Check the secret is stored:

```bash
sudo underwrapctl --socket /run/underwrap-default/control.sock \
  status gh-authorized
# Should show "secret_stored": true
```

## Next steps

- [Configuration](configuration.md) — multi-instance, unlock, OAuth derivation
- [Remote backend](remote-backend.md) — keep secrets off the target machine entirely
- [Security](security.md) — understand every protection mechanism

## Troubleshooting

### "peer egid does not map to any configured wrapper"

The daemon couldn't match the connecting process's effective GID to a wrapper.
This means the process didn't go through a setgid wrapper binary. Check:
- Is the wrapper installed? `ls -la /run/wrappers/bin/<instance>-<wrapper>`
- Does it have the setgid bit? Look for `s` in the group permissions.
- Is the group correct? The group name should be `underwrap-<instance>-<wrapper>`.

### "underwrap-ns not found"

The PID namespace binary isn't at the expected path. Check:
- `ls -la /run/wrappers/bin/underwrap-ns`
- It should have `cap_sys_admin`: `getcap /run/wrappers/bin/underwrap-ns`

### "unshare(CLONE_NEWPID) failed: Operation not permitted"

`underwrap-ns` doesn't have `cap_sys_admin`. This can happen if:
- `/run/wrappers` is mounted with `nosuid` (shouldn't happen on NixOS)
- The binary was copied without capabilities (rebuild should fix this)

### Wrapper just hangs

The daemon might not be running. Check:
`systemctl status underwrapd-<instance>.service`

### "connecting to daemon" error

The read socket doesn't exist or isn't accessible. Check:
- `ls -la /run/underwrap-<instance>/read.sock`
- The socket should be mode 0666.

### Naming convention

Binary names and daemon wrapper names differ:
- **Binary name** (what you type): `/run/wrappers/bin/<instance>-<wrapper>`
- **Daemon wrapper name** (in config, status, unlock): `<wrapper>` (without instance prefix)
- **Group name**: `underwrap-<instance>-<wrapper>`
- **Service name**: `underwrapd-<instance>.service`
- **Control socket**: `/run/underwrap-<instance>/control.sock`

Example for instance `work`, wrapper `gh-authorized`:
- Binary: `/run/wrappers/bin/work-gh-authorized`
- Daemon name: `gh-authorized`
- Group: `underwrap-work-gh-authorized`
- Unlock: `underwrapctl --socket /run/underwrap-work/control.sock unlock gh-authorized`
