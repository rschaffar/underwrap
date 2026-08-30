# Configuration

underwrap has two configuration surfaces:

1. the NixOS frontend module (`services.underwrap`);
2. the YAML file read by `underwrap-backend`.

The NixOS module generates `underwrapd`'s internal YAML and all privileged frontend state.
The generated configuration is trusted integrity state, not confidential storage, and read-only service access protects it from modification rather than disclosure.
Operators should configure the module rather than hand-writing daemon config on NixOS.

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
| `targetRuntimeDirBase` | string | `/run/underwrap-runtime` | Base for one `0700`, target-owned runtime directory per `(instance, wrapper)` target UID; must be canonical absolute and outside `/run/user` |
| `hardenTargetHost` | bool | `true` | Require Linux 6.5+, default Yama `ptrace_scope=2` with `mkDefault`, and admit only no swap, ordinary swap admitted by the random-key/LUKS assertion, or pure RAM-only zram |
| `instances.<name>` | attribute set | `{}` | Named frontend instances; names match `[a-z][a-z0-9_-]{0,20}` |

The security claim in [Security](security.md#security-claim) assumes
`hardenTargetHost = true`. In that posture, an enabled typed
`services.zram-generator.settings` swap section with `writeback-device` is
rejected because backing-device encryption cannot be established from typed
configuration. The predicate permits writeback for canonical mounted zram
filesystem sections and ignores canonical non-zram sections because neither is
classified as memory swap. When zram-generator is enabled, all final typed
section and directive names must nevertheless be canonical tokens and all string
values, including those in mounted and non-zram sections, must be single-line.
The module evaluates typed NixOS configuration; it does not inspect arbitrary
external `/etc` drop-ins or live runtime device state.
Disabling `hardenTargetHost` explicitly selects a weaker deployment posture.

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
| `backend.caFile` | path or string or null | `null` | Canonical absolute path to the complete CA bundle for backend server verification; when set it replaces bundled WebPKI roots, and null uses reqwest's bundled WebPKI roots |
| `wrappers` | attribute set | `{}` | Wrapper definitions keyed by names matching `[A-Za-z0-9][A-Za-z0-9._+-]{0,127}`; `debug` is reserved |

The NixOS module rejects an instance whose name does not match `[a-z][a-z0-9_-]{0,20}`.
It rejects a wrapper whose name does not match `[A-Za-z0-9][A-Za-z0-9._+-]{0,127}` or whose name is the reserved `debug` token.
Invalid selectors do not participate in generated users, groups, shims, daemon files, or target-UID rows.

The NixOS module rejects an instance whose `backend.url` does not start with
lowercase `https://`. `underwrapd` independently parses the URL, requires an
absolute HTTPS URL with a host before reading credentials or binding its socket,
and constructs an HTTPS-only request client. There is no plaintext override,
including for loopback or a proxy upstream. An HTTPS proxy endpoint is admitted;
that admission does not add forwarded-certificate or proxy-identity support.
See [Remote backend](remote-backend.md#https-endpoint-and-proxy-topology).
When `backend.caFile` is set, it must be a canonical absolute path and that bundle is the runtime client instance's exclusive server trust set.
Omitting it uses reqwest's bundled WebPKI roots.
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

The generated full daemon configuration at `/etc/underwrap/daemon/<instance>.yaml` uses canonical absolute paths for `socket`, `debug_tool_target`, `target_runtime_dir_base`, and `backend.client_identity_file`.
The root-generated strict entry-stage sidecar at `/etc/underwrap/instances/<instance>.yaml` contains only `socket`, `ns_binary`, `dbus_daemon_binary`, and `secretsd_binary`.
`underwrap-exec` reads that sidecar, and its `ns_binary` value is the sole configuration authority for the namespace helper path.
The module also requires canonical absolute paths for each wrapper `target`, optional `backend.ca_file`, and forced `env_set` home/base-XDG value.
`underwrapd` applies the same lexical checks to every listed full-daemon runtime field, including hand-written daemon configuration, without requiring the path to exist.
Backend URLs, resource names, non-path `envSet` values, and `envSet.PATH` retain their separate lexical contracts.

### Runtime client credential

The mTLS client identity is generated after deployment, not supplied as a Nix
option. It is one private combined certificate-chain-and-key PEM configured as
`backend.client_identity_file` at:

```text
/var/lib/underwrap-<instance>/client-<instance>.identity.pem
```

Generate it with:

```bash
sudo underwrap-keygen \
  --instance-config /etc/underwrap/daemon/<instance>.yaml
```

The file is installed with mode `0400`. Register the public certificate PEM
printed after installation on a backend runtime client instance; the private key
remains only in the combined identity on the frontend host. The normative
installation and failure guarantees are in
[Security](security.md#runtime-client-identity-installation).

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
| `target` | string | required | Canonical absolute daemon-chosen target path for real invocations |
| `resource` | string | required | Operator-facing backend resource name; one resource per wrapper |
| `env` | bool | `false` | Project the resource's env slot into the target environment |
| `files` | null or submodule | `null` | Enable files projection at `files.paths` |
| `files.paths` | non-empty list of strings | required when `files` is set | Canonical target-home-relative, unique, non-overlapping mount paths |
| `libsecret` | bool | `false` | Project the libsecret slot through a private Secret Service |
| `accessPaths` | list of strings | `[]` | Caller-owned directories exposed through one-pair idmapped bind mounts; entries are canonical absolute paths, exact `~`, or canonical `~/relative` paths |
| `envPassthrough` | list of strings | `[]` | Additional caller environment names beyond the built-in passthrough set |
| `envSet` | attribute set of strings | `{}` | Fixed trusted non-secret target environment values |

`envSet` is materialized in Nix-generated daemon configuration and must not contain brokered values or other secrets.
Put brokered values in resource slots instead.

Every wrapper must enable at least one of `env`, `files`, or `libsecret`.

Canonical path syntax rejects whitespace, controls, empty components, trailing separators, and components equal to `.` or `..`.
Lexical validation does not require filesystem existence and does not resolve symlinks.

The module rejects:

- non-canonical or non-absolute target paths;
- empty resource names;
- non-canonical, duplicate, or overlapping files paths;
- invalid, duplicate, or same-domain overlapping access paths;
- `LD_*` variables in `envPassthrough` or `envSet`;
- forced home/XDG names in `envPassthrough`;
- `XDG_RUNTIME_DIR` in `envSet`;
- non-canonical or non-absolute `HOME` or base XDG values in `envSet`.

### Resource size and transport contract

Every wrapper resource is one complete JCS-canonical blob with an inclusive
2,097,152-byte cap. This is independent of HTTP base64/JSON envelopes and the
inclusive 1,048,576-byte private libsecret publication-body cap. Admin mutations
that would exceed the complete-resource cap fail with HTTP 400
`resource_too_large`; runtime writeback uses its separate stable
`resource-too-large` invalid-blob reason and leaves the prior resource in place.

The authenticated fd-20 Checkout/Checkin channel carries complete resources
under its dedicated bulk bound. Wrapper selection, fd-3 execution metadata, and
other local control messages remain limited to 64 KiB. Resource env values
travel only in authenticated Checkout and are selected into the cleared target
environment; they are not copied through control metadata. A libsecret ACK means
that the host retained a validated candidate, not that the D-Bus caller received
success or that the backend durably saved it. See [Security](security.md#resource-projections)
and [Backend](backend.md#resource-blob-storage) for the normative lifecycle and
persistence rules.

### Target environment

The target starts from `env_clear()`.

The built-in passthrough set includes basic user, terminal, locale, timezone,
temporary-directory, display, SSH-agent, and proxy variables.
It is fixed for all wrappers.
`envPassthrough` can add names; it cannot remove built-ins.
`PATH` is not a built-in and should be set through a trusted `envSet.PATH` when
required.

Forced paths are:

- `HOME` — the original caller's passwd home path unless `envSet.HOME` supplies another canonical absolute path;
- `XDG_RUNTIME_DIR` — the dedicated target UID's provisioned runtime directory;
- `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, and `XDG_STATE_HOME` — derived from effective `HOME` unless explicitly set to canonical absolute values.

Resource env entries cannot use any of these six exact names.
The admin API rejects a per-key write, delete, or rename destination using a forced name with HTTP 400 `invalid_request`.
An env-enabled wrapper fails closed if directly injected resource state contains a forced name, while a wrapper with env projection disabled ignores that env entry.
For ordinary admissible names, resource env values retain precedence over `envSet` and passthrough values.
F-011 content blockers prevent normal deletion of a resource containing such an entry, and no admin repair or migration operation is available.
This outside-contract state requires a quiesced backend and trusted backend-host restoration or replacement of the affected resource from known-good state, or an affected-store rebuild from known-good inputs.
No automated recovery procedure is provided.

The dedicated target UID does not automatically gain access to the caller's home merely because `HOME` names it.
Use `accessPaths` for caller-owned state the target needs.
Files projection mount targets remain anchored to the caller's passwd home path even when `envSet.HOME` points elsewhere.

### Files projection and access paths

Files-slot keys are full paths relative to the caller-home view, for example:

```text
.config/gh/hosts.yml
```

Each key must have one configured `files.paths` prefix.
Configured paths are canonical relative paths, and duplicate or ancestor/descendant entries are rejected before routing.
underwrap aborts when a key has no route.

`accessPaths` serves a different purpose: it exposes existing caller-owned host directories to the dedicated target UID.
Each entry is a canonical absolute path, exact `~`, or a canonical `~/...` path.
Duplicates and ancestor/descendant overlaps are rejected within the absolute domain and separately within the tilde domain.
An absolute path is not compared with a tilde path before caller-home resolution, so apparent cross-domain overlap is neither rejected nor claimed absent at daemon startup.
Each idmapped mount maps only the caller UID/GID to the target UID/GID.
Unsupported nested submounts are skipped with a warning; failure to idmap the named directory itself aborts the invocation.

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

Registry paths have defaults and missing directories are treated as empty.
In a real deployment, set explicit paths and ownership through the service manager.

`https.tls.key_file` names operator-managed persistent authentication material outside resources and `StorageBackend`.
The backend YAML contains paths and settings, not TLS private-key bytes, CA signing-key bytes, or encrypted-store master keys.
The backend reads the configured TLS key but does not install, rotate, atomically replace, or validate its ownership or mode.
The operator and service manager own the TLS-key and any CA-signing-key custody, ownership, mode, umask, and rotation controls.

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
