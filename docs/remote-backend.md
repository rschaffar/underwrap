# Deploying the central backend

underwrap splits its storage from its local enforcement. On any NixOS
machine that runs wrappers, `underwrapd` brokers wrapper identity
locally; it reads and writes secrets by talking to a central
`underwrap-backendd` over HTTPS with mutual TLS.

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

Client certificates are **not** CA-signed. During enrollment the
`underwrap-enroll` helper generates a self-signed leaf locally; the
backend authenticates later mTLS connections by SHA-256 fingerprint
lookup against its runtime-client registry. There is no enrollment CA,
no CSR signing, and no `client_ca_file` to manage.

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
stores_file: /var/lib/underwrap-backend/stores.json
client_instances_file: /var/lib/underwrap-backend/client-instances.json
operators_file: /var/lib/underwrap-backend/operators.json

https:
  listen: 0.0.0.0:18443
  tls:
    cert_file: /etc/underwrap-backend/server.pem
    key_file: /etc/underwrap-backend/server.key

admin:
  session_key_file: /run/keys/underwrap-admin-session-key
```

Provision the admin session key:

```bash
head -c 32 /dev/urandom > /run/keys/underwrap-admin-session-key
chmod 0400 /run/keys/underwrap-admin-session-key
```

Start the daemon with whichever init system suits you. A minimal systemd
unit is in the VM test
(`nix/tests/node.nix`) — copy its `ProtectSystem`, `PrivateTmp`,
`RestrictAddressFamilies`, and `NoNewPrivileges` flags.

## Running `underwrap-enroll`

The helper verifies the backend's server certificate against the host's
**system trust store**. When the backend's cert chains to a public or
otherwise OS-trusted CA, no extra flags are needed.

On NixOS, the `services.underwrap` module symlinks each instance's
generated config to `/etc/underwrap/instances/<instance>.yaml`. Point
`underwrap-enroll` at it and the helper reads the backend URL plus the
client cert/key output paths from that file:

```bash
sudo underwrap-enroll \
  --invite-id enr_… \
  --instance-config /etc/underwrap/instances/work.yaml
```

Off NixOS (or if you want to override the config), pass the targets
explicitly instead:

```bash
sudo underwrap-enroll \
  --url https://backend.example.internal:18443 \
  --invite-id enr_… \
  --client-cert-out /etc/underwrap/client-work.pem \
  --client-key-out /etc/underwrap/client-work.key
```

When the backend's cert is signed by a private CA the host doesn't trust
(or is self-signed), the helper aborts and prints the presented cert plus
its SHA-256 fingerprint. Re-run with `--accept-server-ca-fingerprint
<sha256>` to pin that exact leaf for this enrollment. The pinned
fingerprint is only used by `underwrap-enroll` for this one call; the
runtime daemon still verifies the backend through its own `caFile`
trust anchor.

## Target host (NixOS)

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
      clientCertFile = "/etc/underwrap/client-work.pem";
      clientKeyFile = "/etc/underwrap/client-work.key";
    };

    wrappers.work-gh = {
      target = "${pkgs.gh}/bin/gh";
      resources."github/token".env = true;
    };
  };
};
```

The wrapper appears at `/run/wrappers/bin/work-gh`. Its GID maps to the
`underwrap-work-work-gh` group the module generates; `underwrapd`
derives the wrapper name from `SO_PEERCRED` + the `/proc/<pid>/status`
effective GID.

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
- create the runtime client enrollment for the target instance
- add the wrapper grant and secret material through the UI

## Wrapper approval modes

Wrappers default to "unlocked forever" on first use. Centrally change
that any time:

- **`locked`** — deny everything. Use this for wrappers under
  maintenance or compromise investigation.
- **`unlocked { until_epoch }`** — allow through rules until the given
  epoch; `underwrap-backendd` flips the entry back to `locked`
  automatically.
- **`per-call-approval { approval_key }`** — require the operator to
  sign a fresh nonce with their SSH agent on every invocation. Use this
  for high-privilege wrappers where ambient automation should never be
  able to act alone.

The wrapper-side behavior is identical across modes; only the backend
policy changes.

## What this is not

This page does **not** describe:

- The retired SSH-forwarded Unix socket + `underwrap-backend-ctl export`
  model.
- The retired local `underwrapctl` control socket.
- The retired local timed-unlock flow (`underwrap-exec --approve`
  `--seconds`).

Those shapes no longer exist in the codebase. Any reference in older
commits or old configs will fail at startup today.
