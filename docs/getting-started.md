# Getting started

This guide walks through the simplest useful underwrap setup:

- a central `underwrap-backendd` (here on the same machine, for
  simplicity — the same steps work when it lives on a separate host),
- one NixOS instance with one wrapper,
- one static secret,
- one familiar CLI: [`jira-cli`](https://github.com/ankitpokhrel/jira-cli).

The goal is not to show every feature. It's the smallest path to a
first working wrapper and a working operator UI.

After this, move on to:

- [Agent-facing wrappers](agents.md)
- [GitHub via libsecret](gh-libsecret.md)
- [Deploying the central backend](remote-backend.md)
- [Admin UI](admin-ui.md)

## What you will build

```text
operator browser ─► underwrap-admin-ui ─► underwrap-backendd
                                            ▲
                                            │ mTLS
                                            │
user@console ─► work-jira (setgid) ─► underwrapd-work ─┘
```

When a user runs `work-jira`, underwrap injects `JIRA_API_TOKEN` just
for that invocation and then executes `jira`. The secret is fetched
from the backend the operator configured.

## Prerequisites

- NixOS with a flake-based system configuration.
- The underwrap flake added as an input:

  ```nix
  inputs.underwrap.url = "github:rschaffar/underwrap";
  ```

- A CA + server + client certificate pair for mTLS between the frontend
  daemon and the backend. See
  [Deploying the central backend](remote-backend.md) for one-liner
  recipes with `step-ca`.

## 1. Start the backend

Write `/etc/underwrap-backend/config.yaml`:

```yaml
stores:
  work:
    backend: plain
    dir: /var/lib/underwrap-backend/work

service:
  listen: 127.0.0.1:18443
  tls:
    cert_file: /etc/underwrap-backend/server.pem
    key_file: /etc/underwrap-backend/server.key
    client_ca_file: /etc/underwrap-backend/ca.pem
  clients:
    underwrap-work@localhost:
      store: work

admin:
  listen: 127.0.0.1:19443
  tokens:
    ops:
      token_file: /run/keys/underwrap-admin-ops
```

Provision the admin token and start the daemon:

```bash
head -c 32 /dev/urandom | base64 > /run/keys/underwrap-admin-ops
chmod 0400 /run/keys/underwrap-admin-ops
underwrap-backendd --config /etc/underwrap-backend/config.yaml
```

## 2. Define the frontend instance

In your NixOS configuration:

```nix
{ pkgs, underwrap, ... }:
{
  imports = [ underwrap.nixosModules.default ];

  services.underwrap = {
    enable = true;
    package = underwrap.packages.${pkgs.stdenv.hostPlatform.system}.default;

    instances.work = {
      allowedUsers = [ "alice" ];
      backend = {
        url = "https://127.0.0.1:18443";
        caFile = "/etc/underwrap/ca.pem";
        clientCertFile = "/etc/underwrap/client-work.pem";
        clientKeyFile = "/etc/underwrap/client-work.key";
      };

      credentials."jira/api-token" = {
        mode = "env";
        variable = "JIRA_API_TOKEN";
      };

      wrappers.jira = {
        target = "${pkgs.jira-cli-go}/bin/jira";
        use."jira/api-token".access = "read-only";
      };
    };
  };
}
```

Rebuild:

```bash
sudo nixos-rebuild switch
```

The module creates:

- daemon user/group: `underwrap-work`
- wrapper group: `underwrap-work-jira`
- daemon service: `underwrapd-work.service`
- wrapper socket: `/run/underwrap-work/wrapper-jira.sock`
- setgid wrapper: `/run/wrappers/bin/work-jira`
- wrapper sidecar config: `/etc/underwrap/wrappers/work-jira.yaml`

## 3. Store the secret and authorize the wrapper

The backend starts empty. Put the token in, then add an allow rule so
`work-jira` can read it:

```bash
TOKEN=$(cat /run/keys/underwrap-admin-ops)

# Store the token. Value is base64-encoded bytes.
curl -sS -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"store":"work","secret_id":"jira/api-token",
       "material_b64":"'"$(printf 'your-real-jira-token' | base64)"'"}' \
  http://127.0.0.1:19443/v1/admin/secret/write

# Allow the wrapper to read it.
curl -sS -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"store":"work","rule":{
        "id":"work-jira-read",
        "wrapper":"work-jira",
        "resource":"secret:jira/api-token",
        "operation":"secret.read",
        "effect":"allow"
      }}' \
  http://127.0.0.1:19443/v1/admin/rule/upsert
```

Alternately, run the [admin UI](admin-ui.md) and do it from the browser.

## 4. Initialize `jira-cli`

underwrap only delivers the secret. `jira-cli` still needs its usual
client-side configuration:

```bash
jira init
```

## 5. Run the wrapper

```bash
work-jira issue list
```

This should feel like running `jira issue list`, except the token came
from underwrap.

## Verify the setup

Daemon up?

```bash
systemctl status underwrapd-work.service
```

Wrapper setgid bit?

```bash
ls -l /run/wrappers/bin/work-jira
# → -rwxr-sr-x ... root underwrap-work-jira ...
```

Policy and secret stored?

```bash
curl -sS -H "Authorization: Bearer $TOKEN" \
  -X POST -H "Content-Type: application/json" \
  -d '{"store":"work"}' \
  http://127.0.0.1:19443/v1/admin/policy/get | jq
```

## Common next steps

- **Narrower wrappers for agents** — see
  [Agent-facing wrappers](agents.md) and the
  [wrapper author checklist](wrapper-author-checklist.md).
- **`gh auth login` with stored credentials** — see
  [GitHub via libsecret](gh-libsecret.md).
- **Per-call operator approval for high-privilege wrappers** — set the
  wrapper's mode to `per-call-approval` in the admin UI and register
  your SSH public key. Every invocation then requires a fresh
  ssh-agent signature.

## Troubleshooting

### `peer egid does not map to any configured wrapper`

The process did not reach the daemon through a valid setgid wrapper.
Check that `/run/wrappers/bin/work-jira` exists, is setgid, and is
group-owned by `underwrap-work-jira`.

### `connecting to backend at https://127.0.0.1:18443` fails

The daemon cannot reach the backend, or the mTLS material is wrong.
Check:

```bash
systemctl status underwrap-backendd.service
openssl s_client -connect 127.0.0.1:18443 \
  -CAfile /etc/underwrap/ca.pem \
  -cert   /etc/underwrap/client-work.pem \
  -key    /etc/underwrap/client-work.key
```

### The wrapper runs but Jira still fails to authenticate

underwrap is probably working; `jira-cli` may still be uninitialized or
pointed at the wrong Jira instance. Re-run `jira init`.

## Keep reading

- [Documentation overview](index.md)
- [The underwrap backend](backend.md)
- [Configuration reference](configuration.md)
- [Wrapper author checklist](wrapper-author-checklist.md)
- [Security](security.md)
