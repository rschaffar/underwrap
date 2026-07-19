# Configuration

underwrap has two configuration surfaces:

1. the NixOS frontend module (`services.underwrap`);
2. the YAML file read by `underwrap-backend`.

The NixOS module generates `underwrapd`'s internal YAML and all privileged
frontend state. Operators should configure the module rather than hand-writing
daemon config on NixOS.

## NixOS frontend module

Import `underwrap.nixosModules.default` and configure
`services.underwrap`.

A complete frontend example is in [`examples/nixos.nix`](../examples/nixos.nix).
The generated daemon schema is documented separately in
[`examples/daemon-config.yaml`](../examples/daemon-config.yaml).

### Top-level options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `enable` | bool | `false` | Enable all configured frontend instances |
| `package` | package | required | underwrap package containing the frontend binaries |
| `targetRuntimeDirBase` | string | `/run/underwrap-runtime` | Base for one `0700`, target-owned runtime directory per `(instance, wrapper)` target UID; must be absolute and outside `/run/user` |
| `hardenTargetHost` | bool | `true` | Require Linux 6.5+, default Yama `ptrace_scope=2` with `mkDefault`, and require encrypted swap or no swap; an operator override can weaken the Yama value |
| `instances.<name>` | attribute set | `{}` | Named frontend instances |

The security claim in [Security](security.md#security-claim) assumes
`hardenTargetHost = true`. Disabling it is an explicit weaker deployment
posture.

### Instance options

```nix
services.underwrap.instances.work = {
  allowedUsers = [ "alice" ];

  backend = {
    url = "https://backend.example.internal:18443";
    caFile = "/etc/underwrap/ca.pem";
  };

  wrappers = {
    # …
  };
};
```

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowedUsers` | list of strings | `[]` | Users that receive `<wrapper>` and `debug-<wrapper>` PATH shims; users of an instance literally named `default` also receive an `underwrap` alias |
| `backend.url` | string | required | Absolute backend endpoint in lowercase `https://` form, with a host; non-root base paths and trailing slashes are preserved |
| `backend.caFile` | path or string or null | `null` | Complete CA bundle for backend server verification; when set it replaces bundled WebPKI roots, and null uses reqwest's bundled WebPKI roots |
| `wrappers` | attribute set | `{}` | Wrapper definitions keyed by the canonical wrapper name |

The NixOS module rejects an instance whose `backend.url` does not start with
lowercase `https://`. `underwrapd` independently parses the URL, requires an
absolute HTTPS URL with a host before reading credentials or binding its socket,
and constructs an HTTPS-only request client. There is no plaintext override,
including for loopback or a proxy upstream. An HTTPS proxy endpoint is admitted;
that admission does not add forwarded-certificate or proxy-identity support.
See [Remote backend](remote-backend.md#https-endpoint-and-proxy-topology).
When `backend.caFile` is set, that bundle is the runtime client instance's
exclusive server trust set. Omitting it uses reqwest's bundled WebPKI roots.
This server trust choice is independent of the runtime client's mTLS
certificate and fingerprint identity.

`allowedUsers` is convenience configuration, not access control. The underlying
setgid instance entry point is system-wide, and backend permissions identify the
mTLS runtime client instance rather than the local Unix caller. Use sudo, polkit,
or another OS-level mechanism when local-user authorization is required.

The module assigns each instance:

- daemon user/group `underwrap-<instance>`;
- `/run/wrappers/bin/underwrap-<instance>`;
- `/run/underwrap-<instance>/wrapper.sock`;
- `/var/lib/underwrap-<instance>/`;
- `/etc/underwrap/daemon/<instance>.yaml`;
- `/etc/underwrap/instances/<instance>.yaml`.

### Runtime client credential

The mTLS client credential is generated after deployment, not supplied as a
Nix option. The module configures:

```text
/var/lib/underwrap-<instance>/client-<instance>.pem
/var/lib/underwrap-<instance>/client-<instance>.key
```

Generate it with:

```bash
sudo underwrap-keygen \
  --instance-config /etc/underwrap/daemon/<instance>.yaml
```

Register the printed certificate PEM on a backend runtime client instance. The
private key remains on the frontend host.

### Wrapper options

```nix
wrappers.gh = {
  target = "${pkgs.gh}/bin/gh";
  resource = "github/gh";

  env = true;
  files.paths = [ ".config/gh" ];
  libsecret = true;

  accessPaths = [ "~/.config/git" "~/.ssh" ];
  envSet.PATH = lib.makeBinPath [ pkgs.git pkgs.openssh ];
};
```

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `target` | string | required | Absolute daemon-chosen target path for real invocations |
| `resource` | string | required | Operator-facing backend resource name; one resource per wrapper |
| `env` | bool | `false` | Project the resource's env slot into the target environment |
| `files` | null or submodule | `null` | Enable files projection at `files.paths` |
| `files.paths` | non-empty list of strings | required when `files` is set | Clean, target-home-relative, unique, non-overlapping mount paths |
| `libsecret` | bool | `false` | Project the libsecret slot through a private Secret Service |
| `accessPaths` | list of strings | `[]` | Caller-owned directories exposed through one-pair idmapped bind mounts; entries are `~`, `~/relative`, or absolute paths |
| `envPassthrough` | list of strings | `[]` | Additional caller environment names beyond the built-in passthrough set |
| `envSet` | attribute set of strings | `{}` | Fixed trusted target environment values |

Every wrapper must enable at least one of `env`, `files`, or `libsecret`.
Wrapper name `debug` is reserved.

The module rejects:

- non-absolute target paths;
- empty resource names;
- invalid or overlapping files paths;
- `LD_*` variables in `envPassthrough` or `envSet`;
- forced home/XDG names in `envPassthrough`;
- `XDG_RUNTIME_DIR` in `envSet`;
- relative `HOME` or base XDG values in `envSet`.

### Target environment

The target starts from `env_clear()`.

The built-in passthrough set includes basic user, terminal, locale, timezone,
temporary-directory, display, SSH-agent, and proxy variables. It is fixed for
all wrappers in this release. `envPassthrough` can add names; it cannot remove
built-ins. `PATH` is not a built-in and should be set through a trusted
`envSet.PATH` when required.

Forced paths are:

- `HOME` — the original caller's passwd home path unless `envSet.HOME` supplies
  another absolute path;
- `XDG_RUNTIME_DIR` — the dedicated target UID's provisioned runtime directory;
- `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, and `XDG_STATE_HOME` —
  derived from effective `HOME` unless explicitly set to absolute values.

The dedicated target UID does not automatically gain access to the caller's
home merely because `HOME` names it. Use `accessPaths` for caller-owned state
the target needs. Files projection mount targets remain anchored to the
caller's passwd home path even when `envSet.HOME` points elsewhere.

### Files projection and access paths

Files-slot keys are full paths relative to the caller-home view, for example:

```text
.config/gh/hosts.yml
```

Each key must have one configured `files.paths` prefix. underwrap aborts when a
key has no route or when configured mount paths overlap.

`accessPaths` serves a different purpose: it exposes existing caller-owned host
directories to the dedicated target UID. Each idmapped mount maps only the
caller UID/GID to the target UID/GID. Unsupported nested submounts are skipped
with a warning; failure to idmap the named directory itself aborts the
invocation.

### Debug variant

Every wrapper has an explicit debug invocation form:

```text
underwrap-<instance> debug <wrapper>
debug-<wrapper>              # shim, when installed for the user
```

The daemon selects `underwrap-debug-tool` only when the backend permission mode
is an active `UnsafeDebugUntil`. Local Nix configuration does not authorize
debug access.

## Backend YAML

`underwrap-backend` reads a YAML path passed with `--config`. The annotated
canonical example is [`examples/backend.yaml`](../examples/backend.yaml).

Minimal shape:

```yaml
stores_dir: /var/lib/underwrap-backend/stores
clients_dir: /var/lib/underwrap-backend/clients
users_dir: /var/lib/underwrap-backend/users
bootstrap_file: /var/lib/underwrap-backend/bootstrap.json

https:
  listen: 0.0.0.0:18443
  tls:
    cert_file: /etc/underwrap-backend/server.pem
    key_file: /etc/underwrap-backend/server.key

admin:
  allowed_origins:
    - https://backend.example.internal:18443
  session_lifetime_secs: 43200
  stores_data_root: /var/lib/underwrap-backend/data
```

Registry paths have defaults and missing directories are treated as empty. In a
real deployment, set explicit paths and ownership through the service manager.

`https` is required for a running service. Adding `admin` mounts the admin API
and embedded SPA on the same listener. `stores_data_root` enables store creation
through the admin UI; omitting it leaves existing-store management available
but rejects create-store requests.

Admin sessions are stored in memory and do not survive backend restart.
`allowed_origins` is an additional same-origin check for state-changing
requests; CSRF tokens remain required independently.

Store records, runtime client instances, users, resources, pairs, permissions,
and resource values are managed dynamically through the admin API. They are not
declared in backend YAML.

## Related documentation

- [Getting started](getting-started.md)
- [Admin UI](admin-ui.md)
- [Deploying the backend](remote-backend.md)
- [Backend](backend.md)
- [Security](security.md)
