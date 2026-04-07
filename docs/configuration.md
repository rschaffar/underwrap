# Configuration

## NixOS module

### Single instance

```nix
services.underwrap = {
  enable = true;
  package = underwrap.packages.x86_64-linux.default;

  instances.myproject = {
    wrappers = {
      gws = {
        target = "${pkgs.gws}/bin/gws";
        secret = "google/workspace-creds";
        injection = { mode = "env"; variable = "GOOGLE_WORKSPACE_CLI_TOKEN"; };
        derivation = "oauth_refresh_to_access_token";
        requireUnlock = true;
      };
      gh-authorized = {
        target = "${pkgs.gh}/bin/gh";
        secret = "github/token";
        injection = { mode = "env"; variable = "GH_TOKEN"; };
      };
    };

    secrets = {
      "google/workspace-creds" = { mutable = true; };
      "github/token" = {};
    };
  };
};
```

### Multiple instances

Each instance is fully isolated — separate users, groups, sockets, directories.

```nix
services.underwrap = {
  enable = true;
  package = underwrap.packages.x86_64-linux.default;

  instances = {
    work = {
      remote.enable = true;  # secrets via SSH tunnel
      wrappers.gws = { ... };
      secrets."google/workspace-creds" = { mutable = true; };
    };
    personal = {
      wrappers.gh-authorized = { ... };
      secrets."github/token" = {};
    };
  };
};
```

Instance `work` gets:
- User `underwrap-work`, group `underwrap-work`
- Tunnel user `underwrap-work-tunnel`
- Wrapper group `underwrap-work-gws`
- Service `underwrapd-work.service`
- Runtime dir `/run/underwrap-work/`
- Wrapper binary `/run/wrappers/bin/work-gws`

Instance `personal` gets the same pattern with `personal-` prefix.
No group membership overlaps between instances.

### Remote backend

```nix
instances.myproject = {
  remote.enable = true;
  # Wrappers and secrets as usual — the daemon connects to the
  # tunneled backend socket at /run/underwrap-myproject/backend.sock
};
```

See [remote-backend.md](remote-backend.md) for the full tunnel setup.

### Wrapper options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `target` | string | required | Absolute path to the target binary |
| `secret` | string | required | Secret ID (e.g. `"github/token"`) |
| `injection` | attrs | `{ mode = "env"; variable = "UNDERWRAP_SECRET"; }` | How the secret is delivered |
| `derivation` | string or null | `null` | Derivation mode (e.g. `"oauth_refresh_to_access_token"`) |
| `requireUnlock` | bool | `false` | Require explicit unlock before reads |
| `libsecret` | bool | `false` | Enable private D-Bus Secret Service in the PID namespace |
| `envPassthrough` | list of strings | `[]` | Additional env vars to pass from user's env to target |
| `envSet` | attrs | `{}` | Fixed env vars to set on the target |

### Secret options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `mutable` | bool | `false` | Whether the secret can be overwritten |
| `labels` | attrs | `{}` | Metadata labels |

### Instance options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `wrappers` | attrsOf wrapper | `{}` | Wrapper definitions |
| `secrets` | attrsOf secret | `{}` | Secret definitions |
| `encryption.enable` | bool | `false` | Use encrypted storage |
| `encryption.keyFile` | path | | Master key file path |
| `remote.enable` | bool | `false` | Use remote backend via SSH tunnel |

## YAML configuration (daemon)

The NixOS module generates this automatically. For reference:

```yaml
sockets:
  read: /run/underwrap-myproject/read.sock
  control: /run/underwrap-myproject/control.sock
  read_mode: "0666"
  control_mode: "0600"

storage:
  backend: plain
  dir: /var/lib/underwrap-myproject/secrets
  # Or: backend: remote
  #     socket: /run/underwrap-myproject/backend.sock
  # Or: backend: encrypted
  #     dir: /var/lib/underwrap-myproject/secrets
  #     key_file: /path/to/master.key

wrappers:
  gh-authorized:
    target: /nix/store/.../bin/gh
    secret: github/token
    group: underwrap-myproject-gh-authorized
    injection:
      mode: env
      variable: GH_TOKEN

secrets:
  github/token:
    mutable: false
```

## Wrapper sidecar config

Each wrapper has a sidecar YAML in `/etc/underwrap/wrappers/<instance>-<wrapper>.yaml`:

```yaml
target: /nix/store/.../bin/gh
env_variable: GH_TOKEN
read_socket: /run/underwrap-myproject/read.sock
ns_binary: /run/wrappers/bin/underwrap-ns
env_passthrough: []
env_set: {}
```

The NixOS module generates these. The `env_variable` is derived from
`injection.variable`.

`target` and `ns_binary` are absolute paths. underwrap does not search `PATH`
for the configured main target. For agent-facing use, `target` should usually
point at your own Nix-store wrapper rather than a broad tool directly.

## Storing secrets

```bash
# Store a static token.
echo -n 'ghp_...' | sudo underwrapctl \
  --socket /run/underwrap-myproject/control.sock \
  store github/token

# Store OAuth credentials (multi-line JSON).
sudo underwrapctl --socket /run/underwrap-myproject/control.sock \
  store google/workspace-creds < ~/credentials.json
```

## Unlock management

```bash
# Unlock for 30 minutes (root).
sudo underwrapctl --socket /run/underwrap-myproject/control.sock \
  unlock myproject-gws --seconds 1800

# Self-service unlock via wrapper (SSH agent challenge).
/run/wrappers/bin/myproject-gws --unlock --seconds 1800

# Lock.
sudo underwrapctl --socket /run/underwrap-myproject/control.sock \
  lock myproject-gws
```
