# Getting started

This guide walks through the simplest useful underwrap setup:

- a central `underwrap-backendd` (here on the same machine, for
  simplicity — the same steps work when it lives on a separate host),
- one frontend instance with one wrapper,
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
user@console ─► uw-work jira (setgid) ─► underwrapd-work ─┘
```

When a user runs `jira` (the per-user shim) — equivalently
`uw-work jira` — underwrap injects `JIRA_API_TOKEN` just for that
invocation and then executes the configured `jira` target. The secret is
fetched from the backend the operator configured.

## Prerequisites

- A flake-based NixOS system configuration on the target host.
- The underwrap flake added as an input:

  ```nix
  inputs.underwrap.url = "github:rschaffar/underwrap";
  ```

- A server CA + server certificate for the backend's HTTPS listener (used
  by the frontend daemon to verify the backend). Runtime client certs do
  **not** need a CA — `underwrap-keygen` generates a self-signed leaf
  on the target host and an admin pastes the cert PEM into the backend
  admin UI; the backend authenticates connections by fingerprint
  pinning. See [Deploying the central backend](remote-backend.md) for
  one-liner recipes with `step-ca`.

## 1. Start the backend

Write `/etc/underwrap-backend/config.yaml`:

```yaml
stores_dir: /var/lib/underwrap-backend/stores
clients_dir: /var/lib/underwrap-backend/clients
users_dir: /var/lib/underwrap-backend/users
bootstrap_file: /var/lib/underwrap-backend/bootstrap.json

https:
  listen: 127.0.0.1:18443
  tls:
    cert_file: /etc/underwrap-backend/server.pem
    key_file: /etc/underwrap-backend/server.key

admin:
  stores_data_root: /var/lib/underwrap-backend/data
```

Start the daemon:

```bash
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
      };

      wrappers.jira = {
        target = "${pkgs.jira-cli-go}/bin/jira";
        resource = "jira/api-token";
        env = true;
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

- daemon user and instance group: `underwrap-work`
- daemon service: `underwrapd-work.service`
- the single instance socket: `/run/underwrap-work/wrapper.sock` (0660,
  group `underwrap-work`)
- the one setgid entry point: `/run/wrappers/bin/uw-work`
- per-user shims on the `PATH` of each `allowedUsers` member: `jira` and
  `debug-jira`
- daemon config: `/etc/underwrap/daemon/work.yaml`
- frontend-exec sidecar: `/etc/underwrap/instances/work.yaml`
- expected client credential paths: `/var/lib/underwrap-work/client-work.{pem,key}`
  (generated in the next step)

## 3. Enroll the instance

Until `underwrap-keygen` has populated the per-instance client credential,
the daemon comes up in "unenrolled" mode and prints an enrollment URL on
every wrapper invocation. Generate the credential and register it once:

```bash
sudo underwrap-keygen --instance-config /etc/underwrap/instances/work.yaml
```

The helper writes `/var/lib/underwrap-work/client-work.pem` and
`/var/lib/underwrap-work/client-work.key` (owned by the per-instance
daemon user), then prints the cert PEM to stdout. Open the admin UI,
create a runtime client instance, and paste the PEM. Restart the daemon:

```bash
sudo systemctl restart underwrapd-work
```

## 4. Create the store, store the secret, and authorize the wrapper

The backend starts empty. Open the admin UI in a browser, bootstrap the
first admin operator, create a store named `work`, then:

- add the secret `jira/api-token`
- add a permission allowing wrapper `jira` for the client instance that
  will run it (mode `Unlocked`)

The admin UI is now the canonical way to perform this setup.

## 5. Initialize `jira-cli`

underwrap only delivers the secret. `jira-cli` still needs its usual
client-side configuration:

```bash
jira init
```

## 6. Run the wrapper

```bash
jira issue list          # via the per-user shim
# or, explicitly:
uw-work jira issue list
```

This should feel like running `jira issue list`, except the token came
from underwrap. To inspect the projected environment during an incident,
an operator can set the permission to `UnsafeDebugUntil` and run
`uw-work debug jira` (or `debug-jira`).

## Verify the setup

Daemon up?

```bash
systemctl status underwrapd-work.service
```

Wrapper setgid bit?

```bash
ls -l /run/wrappers/bin/uw-work
# → -rwxr-sr-x ... root underwrap-work ...
```

Policy and secret stored?

Open the admin UI and inspect the `work` store's policy and secrets.

## Common next steps

- **Narrower wrappers for agents** — see
  [Agent-facing wrappers](agents.md) and the
  [wrapper author checklist](wrapper-author-checklist.md).
- **`gh auth login` with stored credentials** — see
  [GitHub via libsecret](gh-libsecret.md).
- **Human-in-the-loop confirmation for high-privilege wrappers** —
  underwrap deliberately does not ship a per-invocation prompt.
  Layer it at the OS level: install the wrapper behind `sudo` (or a
  polkit rule) and rely on the existing audit trail. Pair this with
  a `Locked` permission you flip to `Unlocked` (or `UnlockedUntil`)
  out-of-band for time-bound access windows.

## Troubleshooting

### `peer egid ... does not match instance group gid`

The process did not reach the daemon through the setgid entry point.
Check that `/run/wrappers/bin/uw-work` exists, is setgid, and is
group-owned by `underwrap-work` (the instance group that gates the
socket).

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
