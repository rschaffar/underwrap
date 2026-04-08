# Configuration reference

This document describes the supported NixOS configuration model and the generated runtime layout.

The NixOS module is the primary way to deploy underwrap. If you are just getting started, read [Getting started](getting-started.md) first.

## Top-level module shape

```nix
services.underwrap = {
  enable = true;
  package = underwrap.packages.${pkgs.stdenv.hostPlatform.system}.default;

  instances.<name> = {
    wrappers = { ... };
    secrets = { ... };
    encryption = { ... };
    remote = { ... };
  };
};
```

## Instances

An **instance** is an isolated deployment unit.

Each instance gets its own:

- daemon user and group
- runtime directory
- state directory
- systemd service
- wrapper groups
- optional tunnel user

Example:

```nix
services.underwrap.instances.work = {
  wrappers = { ... };
  secrets = { ... };
};
```

### Generated names

For an instance named `work`, the module creates names following this pattern:

| Item | Example |
|---|---|
| daemon user | `underwrap-work` |
| daemon group | `underwrap-work` |
| daemon service | `underwrapd-work.service` |
| runtime directory | `/run/underwrap-work/` |
| state directory | `/var/lib/underwrap-work/` |
| tunnel directory | `/run/underwrap-work/tunnel/` |
| optional tunnel user | `underwrap-work-tunnel` |

## Wrappers

Each wrapper defines one secret-delivery entry point.

Example:

```nix
wrappers.jira = {
  target = "${pkgs.jira-cli-go}/bin/jira";
  secret = "jira/api-token";
  injection = {
    mode = "env";
    variable = "JIRA_API_TOKEN";
  };
};
```

### Wrapper options

| Option | Type | Default | Meaning |
|---|---|---|---|
| `target` | string | required | Absolute path to the target binary |
| `secret` | string | required | Secret ID to read |
| `injection` | attrset | `{ mode = "env"; variable = "UNDERWRAP_SECRET"; }` | How the secret is injected |
| `derivation` | string or `null` | `null` | Derivation mode, for example `"oauth_refresh_to_access_token"` |
| `requireUnlock` | bool | `false` | Require explicit unlock before reads |
| `libsecret` | bool | `false` | Start a private Secret Service inside the namespace |
| `envPassthrough` | list of strings | `[]` | Additional environment variables to pass through from the caller |
| `envSet` | attrset | `{}` | Fixed environment variables to set for the target |

### Wrapper naming

If your instance is `work` and your wrapper key is `jira`, then:

- the wrapper binary is `/run/wrappers/bin/work-jira`
- the wrapper group is `underwrap-work-jira`
- the wrapper socket is `/run/underwrap-work/wrapper-jira.sock`
- the generated sidecar config is `/etc/underwrap/wrappers/work-jira.yaml`
- the runtime config key seen by the daemon is `work-jira`

That last point matters for commands such as:

```bash
sudo underwrapctl --socket /run/underwrap-work/control.sock status work-jira
```

## Secrets

The `secrets` section defines metadata and mutability for secret IDs.

Example:

```nix
secrets."jira/api-token" = {
  mutable = false;
};
```

### Secret options

| Option | Type | Default | Meaning |
|---|---|---|---|
| `mutable` | bool | `false` | Whether the secret may be overwritten |
| `labels` | attrset of strings | `{}` | Optional metadata labels |

## A realistic multi-wrapper instance

```nix
services.underwrap.instances.work = {
  wrappers = {
    jira = {
      target = "${pkgs.jira-cli-go}/bin/jira";
      secret = "jira/api-token";
      injection = {
        mode = "env";
        variable = "JIRA_API_TOKEN";
      };
    };

    gh = {
      target = "${pkgs.gh}/bin/gh";
      secret = "github/bootstrap";
      libsecret = true;
      injection = {
        mode = "env";
        variable = "GH_UNUSED";
      };
    };

    gws = {
      target = "${pkgs.gws}/bin/gws";
      secret = "google/workspace-creds";
      injection = {
        mode = "env";
        variable = "GOOGLE_WORKSPACE_CLI_TOKEN";
      };
      derivation = "oauth_refresh_to_access_token";
      requireUnlock = true;
    };
  };

  secrets = {
    "jira/api-token" = { mutable = false; };
    "github/bootstrap" = { mutable = false; };
    "google/workspace-creds" = { mutable = true; };
  };
};
```

## Multiple instances

You can run multiple isolated instances on the same machine.

```nix
services.underwrap.instances = {
  work = {
    wrappers = { ... };
    secrets = { ... };
  };

  personal = {
    wrappers = { ... };
    secrets = { ... };
  };
};
```

This is useful when you want hard separation between projects, users, or trust boundaries.

## Encrypted local storage

To enable encrypted local storage:

```nix
instances.work = {
  encryption.enable = true;
  encryption.keyFile = /var/lib/underwrap-work/master.key;

  wrappers = { ... };
  secrets = { ... };
};
```

When encryption is enabled, the generated daemon config uses the encrypted backend instead of the plain backend.

## Remote backend mode

To use the remote backend:

```nix
instances.work = {
  remote.enable = true;

  wrappers = { ... };
  secrets = { ... };
};
```

In this mode, the frontend daemon expects the forwarded backend socket at:

```text
/run/underwrap-work/tunnel/backend.sock
```

The wrapper configuration itself stays the same; only the backend changes.

See [Remote backend](remote-backend.md).

## Environment handling

underwrap starts the target from a cleaned environment.

You can then add controlled environment entries in two ways.

### `envPassthrough`

Pass selected variables from the caller's environment through to the target.

```nix
envPassthrough = [ "EDITOR" "VISUAL" ];
```

### `envSet`

Set fixed variables from trusted configuration.

```nix
envSet = {
  GH_NO_UPDATE_NOTIFIER = "1";
};
```

This is often useful for agent-facing wrappers where you want a predictable target environment.

## Wrapper sidecar config

The module generates one sidecar config per wrapper under:

```text
/etc/underwrap/wrappers/<instance>-<wrapper>.yaml
```

Those files contain the information `underwrap-exec` needs at runtime, such as:

- the target binary path
- the env variable name
- the wrapper socket path
- the shared `underwrap-ns` path
- optional libsecret helper paths

These files are generated from the NixOS configuration and should normally not be edited manually.

## Storing secrets

Store a secret through the control socket:

```bash
sudo underwrapctl --socket /run/underwrap-work/control.sock store jira/api-token
```

`underwrapctl` reads from standard input until EOF.

## Status, unlock, and lock

Examples:

```bash
sudo underwrapctl --socket /run/underwrap-work/control.sock status
sudo underwrapctl --socket /run/underwrap-work/control.sock status work-jira
sudo underwrapctl --socket /run/underwrap-work/control.sock unlock work-gws --seconds 1800
sudo underwrapctl --socket /run/underwrap-work/control.sock lock work-gws
```

## Manual YAML configuration

The NixOS module generates the daemon config automatically. That is the normal deployment path.

If you run components manually, the lower-level daemon configuration supports the same overall ideas: sockets, storage backend, wrappers, and secrets.

The repository includes:

- [`examples/config.yaml`](../examples/config.yaml)
- [`examples/wrappers/gh-authorized.yaml`](../examples/wrappers/gh-authorized.yaml)

These are useful as reference material, especially when looking at how the generated pieces fit together.

## One important limitation

The current NixOS module surfaces the main deployment-oriented options. Lower-level daemon capabilities may exist in the protocol or manual config before they are exposed cleanly through the module.

When in doubt, treat the NixOS module as the authoritative user-facing interface.

## Related docs

- [Getting started](getting-started.md)
- [Architecture](architecture.md)
- [Remote backend](remote-backend.md)
- [Security](security.md)
