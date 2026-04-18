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

mTLS is the backend's one true authentication. You need:

- a CA certificate used to sign both server and client certificates,
- a server certificate whose SANs include the address operators and the
  target host will use (`localhost`, an internal hostname, or an IP),
- one client certificate per target instance. The **subject CN** is the
  instance principal the backend will recognize.

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

step certificate create \
  "underwrap-work@vm-01" client-work.pem client-work.key \
  --profile leaf --ca ca.pem --ca-key ca.key \
  --not-after 8760h --insecure --no-password
```

Copy `ca.pem`, `server.pem`, and `server.key` to the backend host. Copy
`ca.pem`, `client-work.pem`, and `client-work.key` to the target host.

## Backend host

Install `underwrap-backendd` (it builds with plain `cargo build -p
underwrap-backendd`, no NixOS wrapper machinery required) and write
`/etc/underwrap-backend/config.yaml`:

```yaml
stores:
  work:
    backend: plain
    dir: /var/lib/underwrap-backend/work

service:
  listen: 0.0.0.0:18443
  tls:
    cert_file: /etc/underwrap-backend/server.pem
    key_file: /etc/underwrap-backend/server.key
    client_ca_file: /etc/underwrap-backend/ca.pem
  clients:
    underwrap-work@vm-01:
      store: work

admin:
  listen: 127.0.0.1:19443
  tokens:
    ops:
      token_file: /run/keys/underwrap-admin-ops
```

Provision the admin token:

```bash
head -c 32 /dev/urandom | base64 > /run/keys/underwrap-admin-ops
chmod 0400 /run/keys/underwrap-admin-ops
```

Start the daemon with whichever init system suits you. A minimal systemd
unit is in the VM test
(`nix/tests/node.nix`) — copy its `ProtectSystem`, `PrivateTmp`,
`RestrictAddressFamilies`, and `NoNewPrivileges` flags.

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

    credentials."github/token" = {
      mode = "env";
      variable = "GH_TOKEN";
    };

    wrappers.work-gh = {
      target = "${pkgs.gh}/bin/gh";
      use."github/token".access = "read-only";
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

The short way:

```bash
# from the backend host
TOKEN=$(cat /run/keys/underwrap-admin-ops)

# Create an allow rule so work-gh can read github/token.
curl -sS -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
        "store": "work",
        "rule": {
          "id": "gh-token-read",
          "wrapper": "work-gh",
          "resource": "secret:github/token",
          "operation": "secret.read",
          "effect": "allow"
        }
      }' \
  http://127.0.0.1:19443/v1/admin/rule/upsert

# Store the actual secret value.
curl -sS -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"store":"work","secret_id":"github/token",
       "material_b64":"'"$(printf 'ghp_…' | base64)"'"}' \
  http://127.0.0.1:19443/v1/admin/secret/write
```

The [admin UI](admin-ui.md) offers the same primitives with a browser.

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
