# Configuration

Configure the privileged frontend through the NixOS module and the backend through its YAML file.
The module generates daemon configuration, entry-stage sidecars, identities, services, and wrappers; do not hand-edit that generated state.
Generated configuration protects integrity, not secrecy: `envSet` and Nix expressions must never contain brokered values, private keys, or other secrets.

See [Getting started](getting-started.md#1-wire-the-flake-and-frontend-module) for enclosing flake argument wiring, [`examples/nixos.nix`](../examples/nixos.nix) for a frontend module, and [`examples/daemon-config.yaml`](../examples/daemon-config.yaml) for the internal generated schema.

## NixOS options

Import `underwrap.nixosModules.default` and supply the flake input as module argument `underwrap`.
All options below are under `services.underwrap`.
The [NixOS module](../nix/modules/underwrap.nix) owns the exact option declarations and assertion predicates.

| Option | Type and default | Meaning |
| --- | --- | --- |
| `enable` | Boolean, `false` | Enable configured frontend instances |
| `package` | Package, required | Package containing the frontend binaries |
| `targetRuntimeDirBase` | String, `/run/underwrap-runtime` | Canonical absolute base outside `/run/user` for target-owned mode-`0700` runtime directories |
| `hardenTargetHost` | Boolean, `true` | Select the hardened host requirements below |
| `instances` | Attribute set, `{}` | Named frontend instances |

### Host posture

The default posture requires Linux 6.5 or newer for pidfd-based wrapper-peer authentication and defaults `kernel.yama.ptrace_scope` to `2` using `mkDefault`.
Yama restricts host-wide unprivileged debugger/tracer attachment, including ordinary `gdb` and `strace` use.
An operator override can lower it, but weakens ATTACH-mode protection; dedicated target UIDs, not Yama, protect READ-mode `/proc` files such as `environ`.

Admitted swap configurations are no swap, ordinary swap satisfying the per-entry encryption check, or pure RAM-only zram.
For every `swapDevices` entry, that check requires `randomEncryption.enable` or `encrypted.enable`; a device pathname alone does not establish encryption.
When `services.zram-generator.enable` is true, an attribute-set section whose name starts with `zram` may specify `writeback-device` only if it has `mount-point` or a canonical `fs-type` other than the exact token `swap`.
Other sections are outside that memory-swap predicate, not evidence about live backing devices.
Every final typed section and directive name must nevertheless match ASCII `[A-Za-z0-9._:+-]+`, and string values must contain neither CR nor LF, including mounted and non-zram sections.

These are typed NixOS configuration checks, not inspection of arbitrary external `/etc` drop-ins or attestation of live devices.
Operators must preserve the posture outside module-managed settings.
Setting `hardenTargetHost = false` explicitly selects weaker protection, including eligibility for the older-kernel peer-authentication path; a failure in the active pidfd path never authorizes fallback.
See [Security](security.md) for the complete claim and prerequisites.

### Instance options

Options below are relative to `instances.<instance>`.

| Option | Type and default | Meaning |
| --- | --- | --- |
| `allowedUsers` | List of strings, `[]` | Existing users receiving wrapper and debug PATH shims; not an ACL |
| `backend.url` | String, required | Absolute HTTPS endpoint with a host |
| `backend.caFile` | Path/string/null, `null` | Canonical absolute path to the exclusive server CA bundle; null uses bundled WebPKI roots |
| `wrappers` | Attribute set, `{}` | Definitions keyed by wrapper selector |

The module requires the lowercase `https://` prefix; the daemon independently validates an absolute HTTPS URL with a host before credential reads or socket binding.
Non-root base paths and trailing slashes are preserved.
Requests cannot downgrade to HTTP, even for loopback.
A configured CA bundle replaces, rather than supplements, bundled WebPKI roots; hostname verification still applies.
An admitted HTTPS proxy URL does not add forwarded-certificate identity support: use direct backend TLS or Layer-4 passthrough as described in [Operations](operations.md#server-tls).

`allowedUsers` installs `<wrapper>` and `debug-<wrapper>` commands.
Users assigned to an instance named `default` also receive an `underwrap` alias.
These names share a per-user PATH namespace across instances: repeating a wrapper name, defining `debug-foo` alongside `foo`, or colliding with the `underwrap` alias can conflict.
Use distinct wrapper names or nonconflicting user assignments, and use `/run/wrappers/bin/underwrap-<instance> <wrapper>` to select an instance explicitly.
Do not rely on PATH precedence to choose a security-sensitive target.
Any local user can execute the system-wide instance entry point independently of shim installation; use an OS-level gate such as sudo or polkit when local-user authorization is required.

### Names and paths

Instance selectors match ASCII `[a-z][a-z0-9_-]{0,20}`.
Wrapper selectors match ASCII `[A-Za-z0-9][A-Za-z0-9._+-]{0,127}`, and the exact wrapper name `debug` is reserved.
Invalid selectors cannot form generated accounts, shims, configuration paths, or target-UID rows.
Resource names are separate nonempty operator-facing names, such as `demo/token`, not Unix paths or wrapper selectors.

Canonical path syntax rejects whitespace and control characters, empty components, trailing separators, and components equal to `.` or `..`.
Absolute `/` is permitted where the option's additional restrictions allow it.
Canonical relative paths have no leading slash; tilde paths are exactly `~` or canonical `~/relative` paths.
Checks are lexical: they do not normalize paths, require existence, resolve symlinks, or prove that two different spellings identify different filesystem objects.

Canonical absolute paths are required for wrapper `target`, `targetRuntimeDirBase`, optional CA files, and fixed HOME/base-XDG overrides.
The generated daemon also validates its socket, debug target, runtime base, and client identity paths, including when its internal YAML is used in development.
The strict entry sidecar contains only `socket`, `ns_binary`, `dbus_daemon_binary`, and `secretsd_binary`; its `ns_binary` selects the namespace stage.
Do not substitute a hand-written privileged wrapper chain.

### Generated identities and paths

For each instance, the module provisions:

| Object | Name or path |
| --- | --- |
| Daemon user/group | `underwrap-<instance>` |
| Daemon service | `underwrapd-<instance>.service` |
| Setgid entry | `/run/wrappers/bin/underwrap-<instance>` |
| Wrapper socket | `/run/underwrap-<instance>/wrapper.sock` |
| Private state directory | `/var/lib/underwrap-<instance>/` |
| Full daemon config | `/etc/underwrap/daemon/<instance>.yaml` |
| Entry sidecar | `/etc/underwrap/instances/<instance>.yaml` |
| Private combined identity | `/var/lib/underwrap-<instance>/client-<instance>.identity.pem` |

Each `(instance, wrapper)` has a separate `underwrap-target-*` UID/GID and a target-owned runtime directory.
The root-owned `/etc/underwrap/target-uids` table is the sole target-identity authority; real and debug variants of one wrapper share that target identity.
The caller's UID is never a fallback target identity.

Client identities are generated after deployment, not supplied through a Nix option.
Use [Runtime client identities](operations.md#runtime-client-identities) for generation, enrollment, replacement, and the mandatory daemon restart.

### Wrapper options

Options below are relative to `instances.<instance>.wrappers.<wrapper>`.

| Option | Type and default | Meaning |
| --- | --- | --- |
| `description` | String, `""` | Public purpose/capability text for `underwrap-info`; never secrets or sensitive identifiers |
| `target` | String, required | Canonical absolute daemon-chosen real target; prefer a reviewed Nix store helper |
| `resource` | String, required | One nonempty backend resource name |
| `env` | Boolean, `false` | Project resource env entries |
| `files` | Null/submodule, `null` | Enable files projection |
| `files.paths` | Nonempty list when enabled | Canonical relative mount paths, unique and non-overlapping |
| `libsecret` | Boolean, `false` | Project a private Secret Service |
| `accessPaths` | List of strings, `[]` | Selected caller-owned directories exposed with idmapped bind mounts |
| `envPassthrough` | List of strings, `[]` | Additional caller environment names |
| `envSet` | String attribute set, `{}` | Fixed trusted non-secret environment values |

Enable at least one of env, files, or libsecret.
Several wrappers can share one resource while retaining separate wrapper/resource pairs and permission rows.
Backend **writable** permission is distinct from projection configuration: files and libsecret remain writable to the target even when their changes will be discarded.

### Public wrapper discovery

The module installs the unprivileged `underwrap-info` helper and a root-owned mode-`0644` `/etc/underwrap/discovery.json` catalog.
The catalog lists all configured wrappers, not only a user's convenience aliases, with optional descriptions, exact ordinary invocation prefixes, configured host access paths and projection-path metadata.
It is a separate allowlisted schema, not a serialization of full daemon configuration; descriptions do not change registration or authorization.
The helper reads only this public catalog and never invokes a target, fetches credentials, contacts the backend, or checks current filesystem access.
See [Agent discovery and usage](agent-usage.md) for commands, the versioned JSON contract, path guidance and the reusable skill export.

### Target environment

The target starts with `env_clear()`.
For ordinary admissible names, values are applied in ascending precedence: built-in passthrough, `envPassthrough` additions, fixed `envSet`, then projected resource env.
The fixed built-in passthrough set is:

```text
USER LOGNAME SHELL TERM COLORTERM LANG
LC_ALL LC_COLLATE LC_CTYPE LC_MESSAGES LC_MONETARY LC_NUMERIC LC_TIME
TZ TMPDIR SSH_AUTH_SOCK DISPLAY WAYLAND_DISPLAY
http_proxy https_proxy no_proxy HTTP_PROXY HTTPS_PROXY NO_PROXY
```

`envPassthrough` cannot remove built-ins per wrapper.
Reachable proxy, socket, display, and temporary-directory settings can steer a trusted target even though passing a path grants no additional filesystem access.
`PATH` is not built in; use a fixed trusted `envSet.PATH` when required, and review descendants as well as the top-level executable.
The module rejects `LD_*` settings in both `envPassthrough` and `envSet`.

Six paths are forced: `HOME`, `XDG_RUNTIME_DIR`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, and `XDG_STATE_HOME`.
None can be caller passthrough or a resource env name.
`HOME` defaults to the caller's passwd home; the four base XDG paths default below the effective HOME.
Trusted `envSet` may override HOME and those four base paths with canonical absolute values, but may not override `XDG_RUNTIME_DIR`, which is the dedicated target's provisioned directory.

Projected env values must be UTF-8 and contain no NUL, although backend resources can store arbitrary bytes.
Final environment keys cannot contain `=` or NUL, and final values cannot contain NUL.
An env-enabled wrapper rejects invalid stored env names before projection; disabling env skips env-name interpretation, not complete typed resource parsing.
The admin API rejects forced-name writes, deletes, and rename destinations, so outside-contract stored forced names require [trusted-host recovery](operations.md#invalid-stored-environment-recovery), not an ordinary delete workaround.

`HOME` naming the caller's home grants no access to it, and the caller's `/run/user/<uid>` is not automatically exposed.
Files projection remains anchored to the caller's passwd home even if `envSet.HOME` changes.

### Files and explicit host access

A files key is a full caller-home-relative path, such as `.config/tool/credentials`.
It must route below exactly one configured `files.paths` prefix, cannot equal the mount path, and cannot form a file/descendant collision.
The whole routing plan is validated before projection effects.
Staging uses one private size-capped `noswap` tmpfs and retained directory handles, without a shared `/tmp` staging mountpoint.
A mode-`0000` `.underwrap-files` attachment beneath the dedicated target's mode-`0700` runtime directory anchors the tmpfs only in the invocation's private namespace.
Runtime and attachment descriptors require exact target ownership and modes; the host sees only the bare attachment, never staged contents.
Materialization, binding and collection use the retained objects; collection admits regular files and rejects symlinks.
Files setup requires the Linux 6.5 mount/permission APIs and mandatory `noswap` support even when host hardening is disabled; unsupported operations reject without a pathname fallback.

`accessPaths` exposes existing host directories, not backend-managed secret storage.
Entries may be canonical absolute paths, `~`, or canonical `~/...` paths.
Duplicates and ancestor/descendant overlaps are rejected separately within the absolute domain and the tilde domain.
Cross-domain apparent overlaps are not compared before caller-home resolution, so startup does not guarantee their absence.
The mount maps exactly the caller UID/GID to the target UID/GID, and target writes reach the underlying host filesystem as caller-owned state.
The named filesystem must support idmapped mounts or invocation fails; unsupported nested submounts are skipped with a warning.
Grant the narrowest directories and review their configuration, hooks, plugins, and writable state.
See [Wrappers](wrappers.md#choose-a-projection) and [Local protocol](local-protocol.md) for design and realization details.

### Resource limits and debug

A complete canonical resource has an inclusive 2,097,152-byte limit.
The canonical private libsecret publication body has a separate inclusive 1,048,576-byte limit; other local control frames remain limited to 65,536 bytes.
Being below the resource limit does not by itself guarantee files projectability or libsecret readiness.
See [Backend](backend.md) for resource semantics and [Local protocol](local-protocol.md) for framing and publication.

Every wrapper has an explicit `underwrap-<instance> debug <wrapper>` invocation and, where installed, a `debug-<wrapper>` shim.
Only an active backend `UnsafeDebugUntil` permission authorizes the debug target.
See [Unsafe debug](operations.md#unsafe-debug) before enabling it.

## Backend YAML

Pass the YAML path explicitly with `underwrap-backend --config PATH`; the CLI default is `/etc/underwrap/backend.yaml`.
The tutorial and Operations use `/etc/underwrap-backend/config.yaml` explicitly.
[`examples/backend.yaml`](../examples/backend.yaml) contains the complete annotated shape.

| Setting | Default or requirement | Purpose |
| --- | --- | --- |
| `stores_dir` | `/var/lib/underwrap-backend/stores` | Per-store registry records, not resource data |
| `clients_dir` | `/var/lib/underwrap-backend/clients` | Runtime client instance registry |
| `users_dir` | `/var/lib/underwrap-backend/users` | Operator password-verifier records |
| `bootstrap_file` | `/var/lib/underwrap-backend/bootstrap.json` | Offline first-use sentinel |
| `https.listen` | Required for a running listener | Bind address and port |
| `https.tls.cert_file` | Required with HTTPS | Server certificate chain path |
| `https.tls.key_file` | Required with HTTPS | Operator-managed private server key path |
| `admin` | Absent | Presence enables the admin API and embedded SPA on the HTTPS listener |
| `admin.allowed_origins` | `[]` | Additional Origin/Referer check; empty disables this check |
| `admin.session_lifetime_secs` | `43200` | Sliding inactivity interval in seconds for the session cookie and in-memory session; [activity semantics](security.md#sessions-and-browser-custody) include authenticated public requests and non-5xx rejected operations inside the session layer |
| `admin.stores_data_root` | Absent | Data root for newly created stores; absence disables creation, not existing-store management |

Missing registry directories are treated as empty, so use explicit paths and verify ownership before starting a real deployment.
Stores, client instances, users, resources, pairs, permissions, and resource values are managed dynamically, not declared in YAML.

The YAML contains paths and settings, not server key bytes or encrypted-store master keys.
The application reads the TLS key but does not install, rotate, atomically replace, or enforce its ownership or mode.
The service deployment must protect TLS keys, password-verifier records, state directories, and trusted configuration with appropriate ownership, permissions, and umask.
Admin sessions disappear on restart, and origin checks do not replace the separate CSRF protection for authenticated mutations.
See [Operations](operations.md) for deployment, bootstrap, key custody, and recovery.
