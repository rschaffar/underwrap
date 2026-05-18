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
operator browser ─► underwrap-backendd
                         ▲
                         │ HTTPS + runtime client certs
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

- A server CA + server certificate for the backend's HTTPS listener (used
  by the frontend daemon to verify the backend). Runtime client certs do
  **not** need a CA — the helper generates a self-signed leaf during
  enrollment and the backend authenticates connections by fingerprint
  pinning. See [Deploying the central backend](remote-backend.md) for
  one-liner recipes with `step-ca`.

## 1. Start the backend

Write `/etc/underwrap-backend/config.yaml`:

```yaml
stores_file: /var/lib/underwrap-backend/stores.json
client_instances_file: /var/lib/underwrap-backend/client-instances.json
operators_file: /var/lib/underwrap-backend/operators.json

https:
  listen: 127.0.0.1:18443
  tls:
    cert_file: /etc/underwrap-backend/server.pem
    key_file: /etc/underwrap-backend/server.key

admin:
  session_key_file: /run/keys/underwrap-admin-session-key
  stores_data_roots:
    - /var/lib/underwrap-backend
```

Provision the admin session key and start the daemon:

```bash
head -c 32 /dev/urandom > /run/keys/underwrap-admin-session-key
chmod 0400 /run/keys/underwrap-admin-session-key
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

## 3. Create the store, store the secret, and authorize the wrapper

The backend starts empty. Open the admin UI in a browser, bootstrap the
first admin operator, create a store named `work`, then:

- add the secret `jira/api-token`
- add a grant allowing wrapper `work-jira` on host `localhost` for the
  user who will run it

The admin UI is now the canonical way to perform this setup.

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

Open the admin UI and inspect the `work` store's policy and secrets.

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
