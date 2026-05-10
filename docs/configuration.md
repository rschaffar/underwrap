# Configuration reference

underwrap is configured in three places:

- **NixOS module** (`services.underwrap`) — generates target-host runtime
  config, systemd units, users, groups, and wrapper binaries.
- **Backend YAML** (`/etc/underwrap-backend/config.yaml`) — what
  `underwrap-backendd` holds in memory at runtime.
- **Admin UI YAML** (`/etc/underwrap/admin-ui.yaml`) — what the operator
  web service needs to serve.

A useful mental model for the current design:

- a **backend resource** always has three slots:
  - `env`
  - `files`
  - `libsecret`
- the **frontend wrapper config** decides which slots to project into the
  target process
- the **backend policy** decides whether a projected slot is readable and,
  for mutable slots, whether it is writable

The shipped design expects exactly one transport: the frontend always
talks to the backend over HTTPS with mTLS; the admin UI always talks to
the backend over HTTP with a bearer token. Anything older that mentions
SSH forwarding, a local control socket, `underwrapctl`, or a timed
unlock CLI is historical and no longer supported by the code.

## NixOS module

### Top-level options

| Option | Type | Default | Notes |
|---|---|---|---|
| `enable` | bool | `false` | Enables the multi-instance daemon machinery |
| `package` | derivation | — | The underwrap package |
| `hardenTargetHost` | bool | `true` | Pins `kernel.yama.ptrace_scope=2` on the target host (via `mkDefault`) and asserts that every configured swap device is encrypted. This is the posture the strong same-UID protection claim depends on. Lower at your own risk. |
| `instances.<name>` | submodule | `{}` | Per-instance configuration |

The build also rejects any wrapper whose `envPassthrough` or `envSet`
contains an `LD_*` variable (e.g. `LD_PRELOAD`, `LD_LIBRARY_PATH`).
Allowing those would let the caller redirect dynamic linking inside
the target and defeat the `NO_NEW_PRIVS` + `env_clear()` hardening.
This assertion is unconditional, not gated on `hardenTargetHost`.

### Per-instance options

```nix
services.underwrap = {
  enable = true;
  package = pkgs.underwrap;

  instances.work = {
    allowedUsers = [ "alice" ];

    # Required: service mode is the only mode.
    backend = {
      url = "https://backend.example.internal:18443";
      caFile = "/etc/underwrap/ca.pem";
      clientCertFile = "/etc/underwrap/client-work.pem";
      clientKeyFile = "/etc/underwrap/client-work.key";
    };

    wrappers = {
      work-gh = {
        target = "${pkgs.gh}/bin/gh";
        resources."github/token".env = true;
      };
      work-gh-auth = {
        target = "${pkgs.gh}/bin/gh";
        resources."github/gh".libsecret = true;
      };
      work-gws = {
        target = "${pkgs.gws}/bin/gws";
        resources."gws/state".files.path = ".config/gws";
      };
    };
  };
};
```

Key points:

- Every instance requires a full `backend = { url; caFile;
  clientCertFile; clientKeyFile; }`. The module asserts this at build
  time.
- Each wrapper ends up at `/run/wrappers/bin/<instance>-<wrapper>`
  installed setgid to a per-wrapper group. This is the canonical entry
  point and is the path NixOS guarantees by virtue of the setgid bit.
- For every user listed in `allowedUsers`, the module also installs a
  bare-named symlink at `/etc/profiles/per-user/<user>/bin/<wrapper>`
  pointing at the prefixed setgid binary above. NixOS already places
  per-user profile bins on each user's `PATH`, so authorized users can
  invoke wrappers by their bare attribute name (`gh`, `bx`, ...) instead
  of typing the instance prefix. The setgid identity, group, and
  sidecar config still come from the prefixed binary the link resolves
  to. If two instances try to expose the same bare name to the same
  user, the build fails at evaluation with a clear collision error.
- `allowedUsers` is enforced by `underwrapd`; the wrapper setgid bit
  makes the wrapper work at all, but the UID check rejects peers not in
  the list.
- Approval mode is **not** on the wrapper config. It lives in the
  backend's policy document and is set through the admin UI or admin
  API (see [Admin UI](admin-ui.md)).

### Resource slots

Every backend resource always has three slots:

| Slot | What it stores | How wrappers project it |
|---|---|---|
| `env` | named env vars as individual entries under `<resource>/env/<VAR>` | `resources."<resource>".env = true` |
| `files` | a directory tree under `<resource>/files/*` | `resources."<resource>".files.path = ...` |
| `libsecret` | Secret Service items under `<resource>/libsecret/*` | `resources."<resource>".libsecret = true` |

For env projection, the wrapper does **not** predeclare individual variable
names. It receives every env entry currently stored in that resource's env
slot. Dangerous names such as `LD_*`, `PATH`, `HOME`, and `UNDERWRAP_*` are
rejected at runtime.

### Wrapper options

| Option | Type | Default | Notes |
|---|---|---|---|
| `target` | str | — | Absolute path to the binary to exec |
| `resources` | attrs | `{}` | Resource-slot projections for this wrapper |
| `envPassthrough` | `[str]` | `[]` | Extra env vars to let through (PATH is intentionally excluded — use `envSet.PATH`) |
| `envSet` | `{str: str}` | `{}` | Fixed env vars to inject |

There is intentionally no local `read-write` option and no local `approval`
option. Read/write authority and approval mode both live centrally in the
backend policy.

## Backend YAML

```yaml
stores:
  work:
    backend: plain
    dir: /var/lib/underwrap-backend/work
  personal:
    backend: encrypted
    dir: /var/lib/underwrap-backend/personal
    key_file: /run/keys/underwrap-personal.key

service:
  listen: 0.0.0.0:18443
  tls:
    cert_file: /etc/underwrap-backend/server.pem
    key_file: /etc/underwrap-backend/server.key
    client_ca_file: /etc/underwrap-backend/ca.pem
  clients:
    # mTLS subject CN → store name
    underwrap-work@vm-01:
      store: work
    underwrap-personal@laptop-02:
      store: personal

admin:
  listen: 127.0.0.1:19443
  tokens:
    alice:
      token_file: /run/keys/underwrap-admin-alice
    bob:
      token_file: /run/keys/underwrap-admin-bob
```

### Store modes

- **plain** — files under `dir` owned by the daemon user.
- **encrypted** — files under `dir` encrypted at rest with a 32-byte key
  read from `key_file`. The key file must be owned by the daemon user
  with mode `0400` or stricter.

### Service listener

- `cert_file` / `key_file` — the server certificate and private key.
- `client_ca_file` — the CA that signs valid client certificates.
- `clients` — exhaustive list of accepted client principals. A
  certificate whose subject CN is not in this map gets 401.

### Admin listener

The admin listener is plain HTTP. Put it behind a local TLS-terminating
reverse proxy when exposed beyond localhost.

- `tokens.<operator>.token_file` — path to a file whose contents are the
  bearer token for that operator. Tokens are trimmed at load time and
  compared constant-time on every request.

## Admin UI YAML

```yaml
listen: 127.0.0.1:8080

session_key_file: /run/keys/underwrap-admin-ui.session

operators:
  alice:
    password_hash_file: /etc/underwrap-admin-ui/alice.phc

backends:
  corp:
    admin_url: http://127.0.0.1:19443
    token_file: /run/keys/underwrap-admin-corp
```

- `session_key_file` must contain exactly 32 random bytes; they sign
  the session cookie. Rotating this key invalidates all existing
  sessions.
- Each `password_hash_file` is an Argon2 PHC-encoded string (starts
  with `$argon2`). See [Admin UI](admin-ui.md) for generation.
- Each `backends.*.token_file` is the matching bearer token declared on
  the backend host.

## Where to look next

- [The underwrap backend](backend.md) — the full admin + runtime API.
- [Admin UI](admin-ui.md) — operator-facing deployment.
- [Deploying the central backend](remote-backend.md) — step-by-step.
- [Getting started](getting-started.md) — first successful wrapper
  invocation.
