# Configuration reference

This document describes the supported NixOS configuration model and the generated runtime layout.

The NixOS module is the primary way to deploy underwrap. If you are just getting started, read [Getting started](getting-started.md) first.

## Top-level module shape

```nix
services.underwrap = {
  enable = true;
  package = underwrap.packages.${pkgs.stdenv.hostPlatform.system}.default;

  instances.<name> = {
    credentials = { ... };
    wrappers = { ... };
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
  credentials = { ... };
  wrappers = { ... };
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

## Credentials

A **credential** is a reusable resource inside an instance.

A credential defines how it is exposed to targets. Today there are three credential modes:

- `env` — inject as an environment variable
- `libsecret` — expose as a private Secret Service namespace
- `directory` — expose a private backend-managed directory in the target's home view

Example:

```nix
credentials."jira/api-token" = {
  mode = "env";
  variable = "JIRA_API_TOKEN";
};

credentials."github/gh" = {
  mode = "libsecret";
};

credentials."gws/state" = {
  mode = "directory";
  path = ".config/gws";
};
```

### Credential options

| Option | Type | Default | Meaning |
|---|---|---|---|
| `mode` | `"env"`, `"libsecret"`, or `"directory"` | required | How the credential is exposed |
| `variable` | string or `null` | `null` | Environment variable name for `mode = "env"` |
| `path` | string or `null` | `null` | Target-home-relative directory path for `mode = "directory"` |

A credential reference is implicitly scoped by the instance behind the scenes, so the same reference string may be reused safely across different instances.

### Directory credentials

A `directory` credential exposes a backend-managed directory tree at a path relative to the target user's real home directory.

Example:

```nix
credentials."gws/state" = {
  mode = "directory";
  path = ".config/gws";
};
```

underwrap materializes that directory inside the wrapper namespace just before exec. For `read-write` bindings it syncs resulting file changes back to the backend after the target exits. For `read-only` bindings it mounts the directory read-only.

The path must be relative and clean. Absolute paths and traversal components such as `..` are rejected.

## Wrappers

Each wrapper defines one underwrap-facing command.

A wrapper binds one or more declared credentials using the `use` map.

Example:

```nix
wrappers.jira = {
  target = "${pkgs.jira-cli-go}/bin/jira";
  use."jira/api-token".access = "read-only";
};
```

### Wrapper options

| Option | Type | Default | Meaning |
|---|---|---|---|
| `target` | string | required | Absolute path to the target binary |
| `use` | attrset | `{}` | Credential usage bindings for this wrapper |
| `unlock.mode` | `"always-unlocked"`, `"always"`, or `"optional"` | `"always-unlocked"` | Wrapper unlock policy |
| `unlock.maxSeconds` | int or `null` | `null` | Maximum unlock duration for `always` / `optional` |
| `unlockKey` | string or `null` | `null` | SSH public key used for optional unlock challenge flow |
| `envPassthrough` | list of strings | `[]` | Additional environment variables to pass through from the caller |
| `envSet` | attrset | `{}` | Fixed environment variables to set for the target |

### Wrapper credential bindings

Each `use."<ref>"` binding says how this wrapper may use that credential.

```nix
use."jira/api-token" = {
  access = "read-only";
};
```

For shared mutable state such as a libsecret namespace or backend-managed directory:

```nix
use."github/gh" = {
  access = "read-write";
};
```

```nix
use."gws/state" = {
  access = "read-only";
};
```

### Unlock policy

Unlock is defined per wrapper, not per credential.

```nix
unlock = {
  mode = "always";
  maxSeconds = 1800;
};
```

Modes:

- `always-unlocked` — wrapper may be used freely
- `always` — wrapper requires an active unlock grant
- `optional` — wrapper may use a timed unlock grant, or fall back to one-shot SSH-agent challenge flow when configured with `unlockKey`

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

## A realistic multi-wrapper instance

```nix
services.underwrap.instances.work = {
  credentials = {
    "jira/api-token" = {
      mode = "env";
      variable = "JIRA_API_TOKEN";
    };

    "github/gh" = {
      mode = "libsecret";
    };

    "gws/state" = {
      mode = "directory";
      path = ".config/gws";
    };
  };

  wrappers = {
    jira = {
      target = "${pkgs.jira-cli-go}/bin/jira";
      use."jira/api-token".access = "read-only";
    };

    gh-auth = {
      target = "${pkgs.gh}/bin/gh";
      use."github/gh".access = "read-write";
      unlock = {
        mode = "optional";
        maxSeconds = 1800;
      };
      unlockKey = "ssh-ed25519 AAAA...";
    };

    gh-pr = {
      target = "/nix/store/...-gh-pr-helper/bin/gh-pr-helper";
      use."github/gh".access = "read-only";
      unlock = {
        mode = "always";
        maxSeconds = 1800;
      };
    };

    gws-auth = {
      target = "${pkgs.gws}/bin/gws";
      use."gws/state".access = "read-write";
      unlock = {
        mode = "optional";
        maxSeconds = 1800;
      };
    };
  };
  };
};
```

## Multiple instances

You can run multiple isolated instances on the same machine.

```nix
services.underwrap.instances = {
  work = {
    credentials = { ... };
    wrappers = { ... };
  };

  personal = {
    credentials = { ... };
    wrappers = { ... };
  };
};
```

## Encrypted local storage

To enable encrypted local storage:

```nix
instances.work = {
  encryption.enable = true;
  encryption.keyFile = /var/lib/underwrap-work/master.key;

  credentials = { ... };
  wrappers = { ... };
};
```

## Remote backend mode

To use the remote backend:

```nix
instances.work = {
  remote.enable = true;

  credentials = { ... };
  wrappers = { ... };
};
```

In this mode, the frontend daemon expects the forwarded backend socket at:

```text
/run/underwrap-work/tunnel/backend.sock
```

## Environment handling

underwrap starts the target from a cleaned environment.

You can then add controlled environment entries in two ways.

### `envPassthrough`

Pass selected variables from the caller's environment through to the target.

```nix
envPassthrough = [ "EDITOR" "VISUAL" ];
```

Use this sparingly. These values remain caller-controlled.

> [!IMPORTANT]
> `PATH` is not inherited by default. If a wrapper needs helper lookup by bare
> name, prefer absolute paths in the target/helper. If you must provide a
> `PATH`, set it explicitly from trusted config with `envSet.PATH`.
>
> The same caution applies to helper-selection variables such as `EDITOR`,
> `VISUAL`, `PAGER`, and `BROWSER`. Passing them through gives the caller a way
> to influence which helper program the target will execute. Prefer trusted
> fixed values via `envSet` when a wrapper needs those variables at all.

### `envSet`

Set fixed variables from trusted configuration.

```nix
envSet = {
  GH_NO_UPDATE_NOTIFIER = "1";
  PATH = lib.makeBinPath [ pkgs.git pkgs.openssh pkgs.less ];
};
```

This is often useful for agent-facing wrappers where you want a predictable target environment. It is the preferred place to set helper-selection variables such as `PATH`, `EDITOR`, `VISUAL`, `PAGER`, and `BROWSER`.

## Wrapper sidecar config

The module generates one sidecar config per wrapper under:

```text
/etc/underwrap/wrappers/<instance>-<wrapper>.yaml
```

Those files contain the information `underwrap-exec` needs at runtime, such as:

- the target binary path
- the wrapper socket path
- the shared `underwrap-ns` path
- optional libsecret helper paths

## Storing credentials

Store an `env` credential through the control socket:

```bash
sudo underwrapctl --socket /run/underwrap-work/control.sock store jira/api-token
```

`underwrapctl` reads UTF-8 text from standard input until EOF.

`directory` and `libsecret` credentials are normally populated by the wrapped tool itself while it is running under underwrap. They are not stored with the plain `store` command.

## Related docs

- [Getting started](getting-started.md)
- [Wrapper author checklist](wrapper-author-checklist.md)
- [Architecture](architecture.md)
- [Remote backend](remote-backend.md)
- [Security](security.md)
