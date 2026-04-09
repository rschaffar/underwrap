# Getting started

This guide walks through the simplest useful underwrap setup:

- one NixOS instance
- one wrapper
- one static secret
- one familiar CLI: [`jira-cli`](https://github.com/ankitpokhrel/jira-cli)

The goal is not to show every feature. The goal is to get your **first successful wrapper** running with as little conceptual overhead as possible.

After this, move on to:

- [Agent-facing wrappers](agents.md)
- [GitHub via libsecret](gh-libsecret.md)
- [Remote backend](remote-backend.md)

## What you will build

A wrapper called:

```bash
work-jira
```

When a user runs it, underwrap will inject `JIRA_API_TOKEN` just for that invocation and then execute `jira`.

## Prerequisites

- **NixOS**
- a flake-based system configuration
- the underwrap flake added as an input

Add underwrap to your `flake.nix`:

```nix
inputs.underwrap.url = "github:rschaffar/underwrap";
```

## 1. Enable underwrap and define one wrapper

In your NixOS configuration:

```nix
{ pkgs, underwrap, ... }:

{
  imports = [ underwrap.nixosModules.default ];

  services.underwrap = {
    enable = true;
    package = underwrap.packages.${pkgs.stdenv.hostPlatform.system}.default;

    # Define one underwrap instance named "work".
    # The instance name becomes part of generated resources such as:
    # - systemd service: underwrapd-work.service
    # - wrapper binary: /run/wrappers/bin/work-jira
    # - runtime dir: /run/underwrap-work/
    instances.work = {
      # Define one wrapper inside the "work" instance.
      wrappers.jira = {
        target = "${pkgs.jira-cli-go}/bin/jira";
        secret = "jira/api-token";
        injection = {
          mode = "env";
          variable = "JIRA_API_TOKEN";
        };
      };

      # Define metadata for the secret ID used by that wrapper.
      # This does NOT store the secret value itself; you store that later
      # through underwrapctl.
      secrets."jira/api-token" = {
        mutable = false;
      };
    };
  };
}
```

Rebuild your system:

```bash
sudo nixos-rebuild switch
```

## 2. What the module creates

For this example, the NixOS module will create:

- daemon user and group: `underwrap-work`
- wrapper group: `underwrap-work-jira`
- daemon service: `underwrapd-work.service`
- wrapper socket: `/run/underwrap-work/wrapper-jira.sock`
- control socket: `/run/underwrap-work/control.sock`
- setgid wrapper: `/run/wrappers/bin/work-jira`
- wrapper sidecar config: `/etc/underwrap/wrappers/work-jira.yaml`

You do not create those pieces manually. The module wires them up for you.

## 3. Store the Jira API token

Store the token through the control socket.

```bash
sudo underwrapctl --socket /run/underwrap-work/control.sock store jira/api-token
```

`underwrapctl` reads from standard input. Paste the token, then press `Ctrl-D`.

> [!TIP]
> Do not put real secrets directly on the shell command line. Pasting into standard input is safer than `echo token | ...`.

## 4. Make sure `jira-cli` itself is initialized

underwrap only delivers the secret. `jira-cli` still needs its normal client-side configuration.

Run this once as the user who will use the wrapper:

```bash
jira init
```

For the simple cloud-token case, `jira-cli` will use `JIRA_API_TOKEN` when it is present. underwrap will provide that variable only while `work-jira` is running.

> [!NOTE]
> If you use an on-premise Jira PAT workflow that also requires `JIRA_AUTH_TYPE=bearer`, you can set that via `envSet` in the wrapper configuration. Keep the getting-started example simple first.

## 5. Run the wrapper

Now run:

```bash
work-jira issue list
```

That should feel like running `jira issue list`, except the token came from underwrap instead of from your shell environment.

## 6. Verify the setup

Check that the daemon is running:

```bash
systemctl status underwrapd-work.service
```

Check that the wrapper exists and is setgid:

```bash
ls -l /run/wrappers/bin/work-jira
```

You should see the wrapper owned by `root`, group `underwrap-work-jira`, with the setgid bit set.

Check underwrap's view of the wrapper:

```bash
sudo underwrapctl --socket /run/underwrap-work/control.sock status work-jira
```

That should show the configured target, secret ID, and whether the secret is stored.

## What you achieved

You now have a working underwrap deployment with:

- kernel-backed wrapper identity via the wrapper socket plus the wrapper's group
- secret delivery at invocation time instead of shell-export time
- a fixed target binary from root-owned configuration
- a clean base to build more opinionated wrappers on top of

## Common next steps

### Move from broad CLI wrapping to narrow wrappers

If an agent or automation system will call the wrapper, you will usually want a narrow helper in front of the full CLI.

Read: [Agent-facing wrappers](agents.md)

Before you call a wrapper design “done”, also read: [Wrapper author checklist](wrapper-author-checklist.md)

### Use `gh` with login flows instead of a plain env var

If you want `gh auth login` and stored credentials to work cleanly, use libsecret mode.

Read: [GitHub via libsecret](gh-libsecret.md)


## Troubleshooting

### `peer egid does not map to any configured wrapper`

The process did not reach the daemon through a valid setgid wrapper.

Check:

- `/run/wrappers/bin/work-jira` exists
- it has the setgid bit
- its group is `underwrap-work-jira`

### `connecting to /run/underwrap-work/wrapper-jira.sock` fails

The daemon is not running or the runtime directory was not created correctly.

Check:

```bash
systemctl status underwrapd-work.service
ls -l /run/underwrap-work/
```

### The wrapper runs but Jira still fails to authenticate

underwrap may be working, but `jira-cli` may still be uninitialized or pointed at the wrong Jira instance.

Check:

```bash
jira init
```

Then try the wrapper again.

## Keep reading

- [Documentation overview](index.md)
- [Configuration reference](configuration.md)
- [Wrapper author checklist](wrapper-author-checklist.md)
- [Security](security.md)
