# Configuration

underwrap has two configuration surfaces:

- **NixOS module** (`services.underwrap`) — defines frontend instances,
  wrappers, the daemon's backend connection, and the wrapper chain. The
  module generates the setgid binary, the instance user/group, the
  socket, the systemd service, and per-user shims.
- **Backend config** (`underwrap-backend`) — central secret storage,
  the HTTPS admin/runtime listener, and the operator-facing admin API.

## NixOS module

The module lives at `nix/modules/underwrap.nix`. See
[`examples/config.yaml`](../examples/config.yaml) for an annotated
daemon-config equivalent, and the module file itself for the full
option schema.

### Top-level options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `enable` | bool | `false` | Enable the frontend service stack on this host |
| `package` | package | flake package | Which underwrap package to install |
| `hardenTargetHost` | bool | `true` | Pin `kernel.yama.ptrace_scope=2` (via `mkDefault`), require encrypted swap, and assert kernel ≥ 6.5 so the daemon's race-free `SO_PEERPIDFD` peer-auth path is always available. This is the posture the same-UID protection claim in `docs/security.md` depends on. |
| `instances.<name>` | submodule | — | One frontend instance per attribute key (see below) |

`hardenTargetHost = true` is the recommended first-release posture.
Lowering it weakens the security claims in `docs/security.md`.

### Per-instance options

```nix
services.underwrap.instances.work = {
  allowedUsers = [ "alice" ];
  backend = {
    url = "https://backend.example.internal:18443";
    caFile = "/etc/underwrap/ca.pem";
  };
  # xdgRuntimeDirBase = "/run/user";  # default
  wrappers = {
    # …
  };
};
```

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowedUsers` | list of str | `[]` | Local users that get per-user shims (`<wrapper>` and `debug-<wrapper>`) on `PATH` |
| `backend.url` | str | — | HTTPS URL of `underwrap-backend` |
| `backend.caFile` | path | — | CA bundle the daemon uses to verify the backend's server cert |
| `xdgRuntimeDirBase` | str | `/run/user` | Base for the target's `XDG_RUNTIME_DIR` (`<base>/<uid>`) |
| `wrappers` | attrset | — | One entry per wrapper (see below) |

The mTLS client credential is **not** a module option. The module
provisions a per-instance state directory and expects the credential at:

- cert: `/var/lib/underwrap-<instance>/client-<instance>.pem`
- key:  `/var/lib/underwrap-<instance>/client-<instance>.key`

Populate it once with:

```bash
sudo underwrap-keygen --instance-config /etc/underwrap/daemon/<instance>.yaml
```

then paste the printed cert PEM into the admin UI on the backend.

### Per-wrapper options

```nix
wrappers.gh = {
  target = "${pkgs.gh}/bin/gh";
  resource = "gh/main";
  env = true;
  files.paths = [ ".config/gh" ".config/git" ];
  libsecret = true;
  accessPaths = [ "~/.config/git" "~/.ssh" ];
  envSet.PATH = lib.makeBinPath [ pkgs.git pkgs.openssh ];
};
```

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `target` | str | — | Absolute path to the binary the daemon execs for `Real` invocations |
| `resource` | str | — | Backend resource id this wrapper projects (one resource per wrapper) |
| `env` | bool | `false` | Inject the resource's env-slot keys as environment variables |
| `files` | nullable submodule | `null` | Mount the resource's files slot at one or more home-relative paths via `files.paths = [ ... ]` |
| `libsecret` | bool | `false` | Run the target inside a private Secret Service session bus backed by the resource's libsecret slot |
| `accessPaths` | list of str | `[]` | Caller-owned directories to expose to the dedicated target UID through idmapped bind mounts (`~`, `~/rel`, or absolute paths) |
| `envPassthrough` | list of str | minimal allowlist | Caller-environment variables to preserve verbatim (does not name `HOME`, `XDG_*`, `LD_*`, or other dynamic-linker variables) |
| `envSet` | attrset | `{}` | Fixed environment variables to set on the target (absolute paths only for `HOME` and base `XDG_*` overrides) |

`target` must always be an absolute trusted path; underwrap does not use
`PATH` lookup for the main exec target. Every wrapper must enable at
least one projection slot (`env`, `files`, or `libsecret`). Wrapped targets run
under a dedicated target UID; use `accessPaths` to grant that UID explicit,
selected access to caller-owned config or state directories. Only the caller's
uid/gid is mapped through those mounts; other owners remain unmapped.

The debug tool is **always** built alongside every wrapper. Whether an
operator can actually invoke it is a runtime decision: set the
`(client_instance, wrapper, resource)` permission to `UnsafeDebugUntil`
in the admin UI, then run `debug-<wrapper>` (or `underwrap-<instance> debug
<wrapper>`).

### Target-environment policy

The wrapper chain launches the target with `env_clear()` and then
restores only the allowlisted variables. The module enforces:

- `HOME` is set to the trusted passwd home unless `envSet.HOME` overrides
  it with an absolute path
- `XDG_RUNTIME_DIR` is set to `<xdgRuntimeDirBase>/<uid>` (an
  `envSet.XDG_RUNTIME_DIR` override is rejected)
- `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, and
  `XDG_STATE_HOME` are derived from the effective `HOME`
- `envPassthrough` may not name `HOME`, any `XDG_*` variable, or any
  dynamic-linker variable (`LD_*`)

Files-slot mount targets are always anchored to the passwd home resolved
by `underwrap-ns`, not to an `envSet.HOME` override. Choosing a non-passwd
`HOME` while also using `files.paths` is an explicit divergence.

## Backend configuration

`underwrap-backend` reads a YAML config (path passed via `--config`).
Minimal shape:

```yaml
stores_dir:    /var/lib/underwrap-backend/stores
clients_dir:   /var/lib/underwrap-backend/clients
users_dir:     /var/lib/underwrap-backend/users
bootstrap_file: /var/lib/underwrap-backend/bootstrap.json

https:
  listen: 0.0.0.0:18443
  tls:
    cert_file: /etc/underwrap-backend/server.pem
    key_file:  /etc/underwrap-backend/server.key

admin:
  # allowed_origins: [ "https://underwrap.example.internal" ]
  # session_lifetime_secs: 43200   # default 12h
  # stores_data_root: /var/lib/underwrap-backend/data
```

Admin sessions are held in memory (`tower_sessions::MemoryStore`); they
do not survive a restart and there is no signing-key file to provision.
Operators re-authenticate after every backend restart.

The admin SPA ships embedded inside the backend binary and is served
from the same HTTPS listener as the JSON admin API at `/v1/admin/*`.
Browser sessions use Argon2id passwords, opaque `__Host-underwrap-session`
cookies, and synchronizer-token CSRF — see [Admin UI](admin-ui.md). There
is no separate admin process and no bearer-token mode.

## See also

- [The underwrap backend](backend.md) — admin and runtime API surface.
- [Admin UI](admin-ui.md) — operator-facing deployment and auth model.
- [Deploying the central backend](remote-backend.md) — step-by-step.
- [Getting started](getting-started.md) — first successful wrapper
  invocation.
