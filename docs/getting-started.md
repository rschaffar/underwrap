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
inputs.underwrap.url = "github:example/underwrap";
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
- The `gh` process runs inside a PID namespace (invisible to other processes)
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
