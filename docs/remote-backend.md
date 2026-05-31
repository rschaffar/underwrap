# Deploying the central backend

underwrap splits its storage from its local enforcement. On the target
host, `underwrapd` brokers wrapper identity locally; it reads and writes
secrets by talking to a central `underwrap-backendd` over HTTPS with
mutual TLS.

This page walks through a two-machine deployment:

- **backend host** — runs `underwrap-backendd`. Holds the real secret
  state and the policy document.
- **target host** — runs `underwrapd` and the wrappers. Connects
  outbound to the backend host.

For the local-only shape, run both on the same machine and point
`underwrapd` at `127.0.0.1`. Everything below still applies.

## Preparing the PKI

You only need a **server-side** CA:

- a CA certificate used to sign the backend's server certificate,
- a server certificate whose SANs include the address operators and the
  target host will use (`localhost`, an internal hostname, or an IP).

Client certificates are **not** CA-signed. The `underwrap-keygen`
helper generates a self-signed leaf locally on the target host; an
admin pastes the resulting cert PEM into the backend admin UI to
register the runtime client instance. The backend authenticates later
mTLS connections by SHA-256 fingerprint lookup against its
runtime-client registry. There is no enrollment CA, no CSR signing, and
no `client_ca_file` to manage.

A minimal `step-ca` recipe:

```bash
step certificate create \
  "underwrap-backend-ca" ca.pem ca.key \
  --profile root-ca --insecure --no-password

step certificate create \
  "backend.example.internal" server.pem server.key \
  --profile leaf --ca ca.pem --ca-key ca.key \
  --san backend.example.internal --san 127.0.0.1 \
  --not-after 8760h --insecure --no-password
```

Copy `ca.pem`, `server.pem`, and `server.key` to the backend host. Copy
`ca.pem` to each target host so its `underwrapd` can verify the backend's
server cert.

## Backend host

Install `underwrap-backendd` (it builds with plain `cargo build -p
underwrap-backendd`, no NixOS wrapper machinery required) and write
`/etc/underwrap-backend/config.yaml`:

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
  stores_data_root: /var/lib/underwrap-backend/data
```

The `admin:` block must be present (even if otherwise empty) for the
admin API and embedded SPA to be mounted — with no `admin:` key the
backend serves only the mTLS runtime API. `stores_data_root` is required
for the "create store" admin action used below.

Start the daemon with whichever init system suits you. A minimal systemd
unit is in the VM test (`nix/tests/node.nix`) — copy its
`ProtectSystem`, `PrivateTmp`, `RestrictAddressFamilies`, and
`NoNewPrivileges` flags.

## Generating a client credential with `underwrap-keygen`

`underwrap-keygen` runs on the target host and never talks to the
backend. It generates a fresh keypair, writes the private key and a
self-signed certificate to disk under the daemon user, and prints the
certificate PEM to stdout. The operator pastes that PEM into the admin
UI on the backend to register the runtime client instance.

On NixOS, the `services.underwrap` module writes each instance's daemon
config to `/etc/underwrap/daemon/<instance>.yaml` — the file that carries
the `backend` section with the client cert/key paths. Point
`underwrap-keygen` at it and the helper reads those output paths plus the
daemon user from that file:

```bash
sudo underwrap-keygen \
  --instance-config /etc/underwrap/daemon/work.yaml
```

Off NixOS (or if you want to override the paths), pass the targets
explicitly instead:

```bash
sudo underwrap-keygen \
  --client-cert-out /etc/underwrap/client-work.pem \
  --client-key-out /etc/underwrap/client-work.key
```

The helper writes the key file with mode `0400` and the cert file with
`0600`, and chowns both to the `underwrap-<instance>` user/group when it
can resolve them.
Copy the PEM that lands on stdout into the admin UI's **Create client
instance** form on the backend; the backend stores the SHA-256
fingerprint and authorises later mTLS connections by it.

## Target host

Configure `services.underwrap` to connect outbound to the backend:

```nix
services.underwrap = {
  enable = true;
  package = pkgs.underwrap;

  instances.work = {
    allowedUsers = [ "alice" ];
    backend = {
      url = "https://backend.example.internal:18443";
      caFile = "/etc/underwrap/ca.pem";
    };

    wrappers.gh = {
      target = "${pkgs.gh}/bin/gh";
      resource = "github/token";
      env = true;
    };
  };
};
```

The module installs the single setgid entry point at
`/run/wrappers/bin/underwrap-work`, group-owned by `underwrap-work`, and a
per-user `gh` shim on each authorized user's `PATH`. The user invokes
`gh` (or `underwrap-work gh`); `underwrapd` verifies the peer's effective GID
against the instance group (race-free via `SO_PEERPIDFD`), validates the
wrapper name against config, and forwards the invocation to the
backend.

Restart the frontend instance service:

```bash
systemctl restart underwrapd-work.service
```

## Putting a wrapper under policy

Out of the box, the backend's policy document is empty. An empty
document means **every request with no matching rule is denied**, so new
wrappers always need at least one allow-rule before they can do
anything.

The short way is now the admin UI in the browser:

- bootstrap the first admin operator
- create the `work` store
- run `underwrap-keygen` on the target host and paste its cert PEM into
  the **Create client instance** form on the **Client Instances** page
- add the wrapper grant and secret material through the UI

## Wrapper approval modes

Permissions default to `Locked` when the backend auto-seeds them on
register. Operators flip them centrally any time:

- **`locked`** — deny everything. Use for wrappers under maintenance or
  compromise investigation; the operator-supplied `reason` field is
  surfaced verbatim to the caller.
- **`unlocked`** — allow every `Real` invocation through this permission.
- **`unlocked-until { until_epoch }`** — allow `Real` invocations until
  the given epoch (unix seconds); the deadline is checked lazily on each
  request, there is no background sweeper.
- **`unsafe-debug-until { until_epoch }`** — allow both `Real` and
  `Debug` invocations until the given epoch. `Debug` runs the debug tool
  instead of the configured target and prints the projected secret
  material to the operator's terminal. Every other mode denies `Debug`
  with `not-unsafe-debug`.

Per-invocation approval is intentionally not a feature of this product —
use `sudo` or polkit in front of the wrapper for human-in-the-loop
confirmation.

The wrapper-side behavior is identical across modes; only the backend
policy changes. Read access through an active permission is
unconditional; write access is decided per-permission via the
`writable: bool` flag and re-checked on every finalize.
