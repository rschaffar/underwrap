# Getting started

This tutorial builds the smallest deterministic underwrap deployment:

- one `underwrap-backend` on the NixOS frontend host;
- one frontend instance;
- one plain store;
- one stateless helper that verifies an env projection without printing it.

The tutorial uses a plain store to keep the first run focused. Use an encrypted
store after the complete flow works.

## Result

The caller runs:

```bash
secret-check
```

The target prints:

```text
DEMO_TOKEN is available to the target
```

The caller's shell never receives `DEMO_TOKEN`, and the helper never prints its
value.

## Prerequisites

- A flake-based NixOS configuration.
- An existing local user; the examples use `alice`.
- The underwrap flake input:

  ```nix
  inputs.underwrap.url = "github:rschaffar/underwrap";
  ```

- A server CA and server certificate for `127.0.0.1` or the hostname used by
  the daemon and browser. [Deploying the backend](remote-backend.md#server-tls)
  includes a `step` recipe.

The browser must trust the server CA. Runtime client certificates use a
separate model: `underwrap-keygen` creates a self-signed leaf and the operator
registers its fingerprint through the admin UI.

## 1. Configure the NixOS frontend

Add the module and a stateless target helper:

```nix
{ pkgs, underwrap, ... }:

let
  secretCheck = pkgs.writeShellApplication {
    name = "underwrap-secret-check";
    text = ''
      set -euo pipefail
      if [[ -z "''${DEMO_TOKEN:-}" ]]; then
        echo "DEMO_TOKEN is missing" >&2
        exit 1
      fi
      echo "DEMO_TOKEN is available to the target"
    '';
  };
in
{
  imports = [ underwrap.nixosModules.default ];

  # Installs the backend command used by this local tutorial.
  environment.systemPackages = [
    underwrap.packages.${pkgs.stdenv.hostPlatform.system}.underwrap-backend
  ];

  services.underwrap = {
    enable = true;
    package = underwrap.packages.${pkgs.stdenv.hostPlatform.system}.default;

    instances.demo = {
      # Replace alice with an existing local user. This installs the
      # `secret-check` and `debug-secret-check` PATH shims; it is not an ACL.
      allowedUsers = [ "alice" ];

      backend = {
        url = "https://127.0.0.1:18443";
        caFile = "/etc/underwrap/ca.pem";
      };

      wrappers.secret-check = {
        target = "${secretCheck}/bin/underwrap-secret-check";
        resource = "demo/token";
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

- `underwrapd-demo.service`;
- `/run/wrappers/bin/underwrap-demo`;
- `/run/underwrap-demo/wrapper.sock`;
- `/etc/underwrap/daemon/demo.yaml`;
- `/etc/underwrap/instances/demo.yaml`;
- `/var/lib/underwrap-demo/` for the runtime client credential;
- one dedicated target UID/GID and runtime directory for `secret-check`;
- the `secret-check` and `debug-secret-check` shims in Alice's per-user
  profile.

The daemon can start before its client credential or backend is available. It
retries backend initialization on each wrapper request.

## 2. Prepare the backend

Create backend state directories:

```bash
sudo install -d -m 0700 \
  /var/lib/underwrap-backend/stores \
  /var/lib/underwrap-backend/clients \
  /var/lib/underwrap-backend/users \
  /var/lib/underwrap-backend/data
```

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
  allowed_origins:
    - https://127.0.0.1:18443
  stores_data_root: /var/lib/underwrap-backend/data
```

Before starting the listener, create the first operator through backend-host
filesystem authority. Run the command as the identity that owns the backend
state and supply the password through standard input from a trusted prompt or
password manager:

```bash
sudo underwrap-backend \
  --config /etc/underwrap-backend/config.yaml \
  admin bootstrap-user --username alice
```

Do not place the password in argv, environment variables, configuration, or
shell history. Then start the backend in a dedicated terminal:

```bash
sudo underwrap-backend \
  --config /etc/underwrap-backend/config.yaml
```

A production deployment should use a hardened service unit; see
[Deploying the backend](remote-backend.md).

## 3. Sign in and create the store

Open:

```text
https://127.0.0.1:18443/
```

Sign in with the offline-provisioned operator, then open **Stores** and create a
**plain** store named `demo`.

The store must exist before a runtime client instance can select it.

## 4. Generate and enroll the runtime client

Generate the per-instance credential on the frontend host:

```bash
sudo underwrap-keygen \
  --instance-config /etc/underwrap/daemon/demo.yaml
```

On a successful run, the command installs one private combined certificate-
chain-and-key identity at
`/var/lib/underwrap-demo/client-demo.identity.pem` with mode `0400`, then prints
only the public certificate PEM. See the normative
[identity installation guarantees](security.md#runtime-client-identity-installation).

In **Client instances**, choose **Create client instance** and set:

- display name: `demo-host`;
- target store: `demo`;
- status: `Active`;
- certificate PEM: the complete output certificate;
- **Automatically create wrapper resources and pairs:** enabled.

Auto-create is convenient for this tutorial. In a tightly controlled deployment,
leave it disabled and create each resource and wrapper/resource pair manually.

Restart the frontend daemon so it authenticates and registers its configured
pair:

```bash
sudo systemctl restart underwrapd-demo.service
```

Registration creates the resource `demo/token`, the pair
`secret-check → demo/token`, and a `Locked` permission row for this runtime
client instance.

For a later certificate replacement, follow the normative
[ordered cutover](admin-ui.md#replace-a-runtime-client-certificate): successfully
run keygen on the frontend, save its public certificate on the same backend
instance, run `sudo systemctl restart underwrapd-<instance>.service`, and verify a
representative wrapper. The old daemon identity remains cached until restart, so
wrapper failure with enrollment guidance after **Save** and before restart is the
expected interruption.

## 5. Add the env entry and unlock the wrapper

In **Secrets**:

1. select the `demo` store;
2. open resource `demo/token`;
3. add an **env** entry named `DEMO_TOKEN`;
4. enter a non-production value for the tutorial.

In **Wrapper permissions**:

1. select the `demo` store;
2. find wrapper `secret-check` and the `demo-host` client instance;
3. change the permission mode from `Locked` to `Unlocked`;
4. leave **writable** off; env entries have no runtime write path.

The exact data model is:

```text
store: demo
resource: demo/token
slot/key: env/DEMO_TOKEN
pair: secret-check → demo/token
permission: (demo-host, secret-check, demo/token) = Unlocked, writable=false
```

## 6. Run the wrapper

As Alice:

```bash
secret-check
```

The explicit instance form is equivalent:

```bash
underwrap-demo secret-check
```

Do not use `debug-secret-check` for routine verification. The debug variant is
available only under a time-bounded `UnsafeDebugUntil` permission and exposes
the projection to an incident-debug session.

## Verify the deployment

Frontend daemon:

```bash
systemctl status underwrapd-demo.service
```

Setgid entry point:

```bash
ls -l /run/wrappers/bin/underwrap-demo
```

Identity ownership and mode:

```bash
sudo ls -ld /var/lib/underwrap-demo
sudo ls -l /var/lib/underwrap-demo/client-demo.identity.pem
```

The admin UI should show:

- one active client instance mapped to `demo`;
- resource `demo/token` with one env entry;
- pair `secret-check → demo/token`;
- one unlocked, non-writable permission row.

## Troubleshooting

### The `secret-check` command is missing

Confirm that the local username appears in `allowedUsers`, then rebuild. This
option installs shims only; the explicit `underwrap-demo secret-check` form is
system-wide.

### `peer egid ... does not match instance group gid`

The request did not traverse the setgid entry point. Verify the mode and group
of `/run/wrappers/bin/underwrap-demo` and do not connect directly to the socket.

### The wrapper says the instance is not enrolled

Check that the combined identity file exists with the expected owner and mode,
and that its public certificate PEM was registered on an active client instance.
A replaced or disabled identity produces the same enrollment guidance.

### The resource or wrapper pair is missing

Confirm that auto-create is enabled for the client instance and restart
`underwrapd-demo`. Otherwise create resource `demo/token` and its
`secret-check` pair manually in the admin UI.

### The permission is locked or missing

Registration seeds a locked permission. Open **Wrapper permissions** and set the
cell for the correct client, wrapper, and resource to `Unlocked`.

### TLS connection fails

Check the backend process, URL, server certificate SAN, and CA file:

```bash
systemctl status underwrapd-demo.service
openssl s_client -connect 127.0.0.1:18443 \
  -CAfile /etc/underwrap/ca.pem
```

## Next steps

- [Admin UI](admin-ui.md) for encrypted stores, manual pair creation, users,
  and debug permissions.
- [Configuration](configuration.md) for wrapper projections and host access.
- [Agent-facing wrappers](agent-facing-wrappers.md) before exposing a wrapper to
  automation.
- [GitHub via libsecret](gh-libsecret.md) for mutable Secret Service state.
- [Deploying the backend](remote-backend.md) to separate backend and target
  hosts.
- [Security](security.md) before using production credentials.
