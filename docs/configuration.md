# Configuration

underwrap has two main layers of configuration:

- **NixOS module** (`services.underwrap`) — generates target-host runtime
  configuration, wrapper binaries, users/groups, sockets, and policy
  fragments
- **Backend config** (`underwrap-backendd`) — central secret storage,
  service clients, admin tokens, and the operator-facing admin API

## NixOS module highlights

The NixOS module lives at `nix/modules/underwrap.nix`.

### Top-level options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `enable` | bool | `false` | Enable the underwrap target-host service stack |
| `package` | package | flake package | Which underwrap package set to use |
| `backendUrl` | str | — | HTTPS URL of the remote backend service API |
| `caFile` | path | — | CA bundle used to verify the backend certificate |
| `instanceNamePrefix` | str | `"underwrap"` | Prefix for generated users/groups/socket paths |
| `hardenTargetHost` | bool | `true` | Pins `kernel.yama.ptrace_scope=2` on the target host (via `mkDefault`) and asserts that every configured swap device is encrypted. This is the posture the strong same-UID protection claim depends on. Lower at your own risk. |

`hardenTargetHost = true` is the recommended first-release posture. It
keeps the host aligned with the security claims in `docs/security.md`.

The mTLS client credential is **not** an operator-configurable option. The
module places it at a per-instance path under the per-instance state dir:

- cert: `/var/lib/underwrap-<instance>/client-<instance>.pem`
- key:  `/var/lib/underwrap-<instance>/client-<instance>.key`

Populate it with `underwrap-keygen --instance-config /etc/underwrap/instances/<instance>.yaml`
and paste the resulting cert PEM into the admin UI. This forecloses the
class of "two instances share the same cert path" / "stale cert was wired
in via deploy" mistakes that fingerprint-pinning can't otherwise detect.

### Why the module clears the target environment

The wrapper chain launches the final target with `env_clear()` and then
reconstructs only an allowlisted environment. This blocks caller-driven:

- `LD_PRELOAD`
- `LD_LIBRARY_PATH`
- shell helper injection
- home/config/runtime-dir steering through `HOME`, `XDG_RUNTIME_DIR`, or
  `XDG_*`
- incidental secret leakage from the caller environment

The target process should only receive the variables explicitly declared
in wrapper configuration.

Under the default policy:

- `HOME` is forced to the trusted passwd home
- `XDG_RUNTIME_DIR` is forced to `<xdgRuntimeDirBase>/<uid>`
- `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, and
  `XDG_STATE_HOME` are derived from the effective `HOME`
- `envSet` may intentionally override `HOME` and the base `XDG_*` dirs,
  but those values must be absolute paths
- `envPassthrough` may not name any of those variables
- `envSet.XDG_RUNTIME_DIR` is rejected; use the instance-level
  `xdgRuntimeDirBase` knob instead

One intentional distinction remains: files-slot mount targets are still
anchored to the passwd home resolved by `underwrap-ns`, not to an
`envSet.HOME` override. If you set `HOME` away from the passwd home for a
wrapper that also uses `files.paths`, you are choosing that divergence
explicitly.

Likewise, `xdgRuntimeDirBase` controls only the exported
`XDG_RUNTIME_DIR` value. With the default `/run/user`, the host session
usually provides the directory tree already. If you choose a custom base,
ensure that the resulting `<base>/<uid>` path exists and is usable by the
target user.

## Example wrapper configuration

```nix
services.underwrap.wrappers = {
  gh-work = {
    resource = "gh/work";
    wrapperGroup = "underwrap-gh-work";
    target = "${pkgs.gh}/bin/gh";
    env = {
      GH_TOKEN = {
        slot = "env";
        entry = "GH_TOKEN";
      };
    };
  };

  gh-personal = {
    resource = "gh/personal";
    wrapperGroup = "underwrap-gh-personal";
    target = "${pkgs.gh}/bin/gh";
    env = {
      GH_TOKEN = {
        slot = "env";
        entry = "GH_TOKEN";
      };
    };
  };

  gws-project = {
    resource = "google/projektbuero";
    wrapperGroup = "underwrap-gws-project";
    target = "${pkgs.gws}/bin/gws";
    libsecret = true;
  };
};
```

## Debug / override helper

The module can also generate a debug wrapper that keeps the real
wrapper's GID and resource projections but swaps the target for a helper
command. This is useful for inspecting the environment and projection
shape without exposing raw secret values in logs.

## Wrapper option reference

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `resource` | str | — | Backend resource id |
| `wrapperGroup` | str | — | Group used for wrapper identity via `getegid()` |
| `target` | str | — | Absolute path to the binary to exec |
| `targetArgs` | list of str | `[]` | Fixed arguments passed to the target |
| `libsecret` | bool | `false` | Launch the target in private Secret Service mode |
| `env` | attrset | `{}` | Env-slot projections for secret material |
| `files` | attrset | `{}` | File-slot projections into the target view |
| `allowedEnv` | list of str | minimal allowlist | Non-secret environment variables that may be preserved |

`target` must always be an absolute trusted path. underwrap does not use
`PATH` lookup for the main exec target.

## Backend admin tokens

`underwrap-backendd` exposes a bearer-token protected admin API for the
operator UI. Each configured token is loaded from a file and matched
constant-time on every request.

## Admin UI runtime YAML

The Next.js admin UI reads its runtime config from the file pointed to by
`UNDERWRAP_ADMIN_UI_CONFIG`.

```yaml

operators:
  alice:
    password_hash_file: /etc/underwrap-admin-ui/alice.phc

backend:
  label: shame01
  admin_url: http://127.0.0.1:19443
  token_file: /run/keys/underwrap-admin-backend
```

- `operators` may be empty or omitted. In that case the admin UI exposes
  the first-use bootstrap flow and stores the created operator hash in
  local state under `UNDERWRAP_ADMIN_UI_STATE_DIR`.
- Each configured `password_hash_file` is an Argon2 PHC-encoded string
  (starts with `$argon2`). See [Admin UI](admin-ui.md).
- `backend.token_file` is the matching bearer token declared on the
  backend host.
- `backend.label` is only the UI's backend/host label. It is not part of
  backend authn/authz.
- Listener address and port are not part of this YAML; the packaged
  Next.js server binds through `HOSTNAME` and `PORT`.

## Where to look next

- [The underwrap backend](backend.md) — the full admin + runtime API.
- [Admin UI](admin-ui.md) — operator-facing deployment.
- [Deploying the central backend](remote-backend.md) — step-by-step.
- [Getting started](getting-started.md) — first successful wrapper
  invocation.
