# Getting started

Run one presence-only helper on one NixOS host, with a local backend and a plain store containing a disposable non-production value.
The helper reports whether its env projection is nonempty without printing the value or putting it in the caller's shell.
This is a first-deployment exercise, not a production service or an end-to-end security audit.
Underwrap is pre-release security software without an independent end-to-end audit.

## Before changing the host

You need an existing flake-based NixOS configuration, root access, an existing local user, and a browser on this host.
The examples use host configuration `demo-host`, system `x86_64-linux`, and user `alice`; substitute your existing names and system.
Keep your existing hardware configuration, boot settings, user definitions, and NixOS state version.

The default hardened posture requires Linux 6.5 or newer and no swap, ordinary swap admitted by the module's random-key/LUKS assertion, or pure RAM-only zram.
Typed zram-generator swap writeback is rejected; the exact typed configuration restrictions and their limits are in [Configuration](configuration.md#host-posture).
The module defaults host-wide `kernel.yama.ptrace_scope` to `2`, which prevents ordinary unprivileged debugger and tracer attachment, including `gdb` and `strace` against other user processes.
Check whether this is acceptable before rebuilding, and do not disable hardening just to finish the tutorial.
Dedicated target UIDs protect READ-mode `/proc` access; Yama adds ATTACH-mode protection rather than protecting `environ` itself.
Read [Security](security.md) before using production credentials.

`allowedUsers` installs PATH shims, not a local-user ACL.
Any local user can invoke the system-wide instance entry point when backend policy permits the runtime client instance and wrapper/resource pair.
The configured target is trusted with its projection, so this tutorial deliberately uses a fixed helper rather than a general secret-printing CLI.

## 1. Wire the flake and frontend module

Merge this enclosing structure into your existing `flake.nix` rather than replacing your host configuration.
Keep your existing `nixpkgs` input and lock file; `specialArgs` is what supplies the `underwrap` argument to the module below.

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    underwrap.url = "github:rschaffar/underwrap";
  };

  outputs = { nixpkgs, underwrap, ... }: {
    nixosConfigurations.demo-host = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      specialArgs = { inherit underwrap; };
      modules = [
        ./configuration.nix
        ./underwrap-demo.nix
      ];
    };
  };
}
```

Create `underwrap-demo.nix` beside it:

```nix
{ pkgs, underwrap, ... }:
let
  secretCheck = pkgs.writeShellApplication {
    name = "underwrap-secret-check";
    text = ''
      if [[ -z "''${DEMO_TOKEN:-}" ]]; then
        echo "DEMO_TOKEN is missing" >&2
        exit 1
      fi
      echo "DEMO_TOKEN is available to the target"
    '';
  };
in {
  imports = [ underwrap.nixosModules.default ];

  environment.systemPackages = [
    underwrap.packages.${pkgs.stdenv.hostPlatform.system}.underwrap-backend
    pkgs.python3
    pkgs.openssl
    pkgs.step-cli
  ];

  # Backend identity for this foreground-only tutorial.
  users.groups.underwrap-backend = {};
  users.users.underwrap-backend = {
    isSystemUser = true;
    group = "underwrap-backend";
  };

  services.underwrap = {
    enable = true;
    package = underwrap.packages.${pkgs.stdenv.hostPlatform.system}.default;
    instances.demo = {
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

Ensure new configuration files are included in your flake source, then rebuild the selected existing host:

```bash
sudo nixos-rebuild switch --flake .#demo-host
```

The frontend module creates `underwrapd-demo.service`, daemon user/group `underwrap-demo`, `/run/wrappers/bin/underwrap-demo`, and `/run/underwrap-demo/wrapper.sock`.
It also creates `/etc/underwrap/daemon/demo.yaml`, `/etc/underwrap/instances/demo.yaml`, the private `/var/lib/underwrap-demo/` state directory, and a dedicated target UID/GID and runtime directory for the wrapper.
Alice receives `secret-check` and `debug-secret-check` shims.
Missing backend credentials are expected at this stage; request-time initialization is not a substitute for the enrollment restart below.

## 2. Place TLS and backend configuration

Obtain a server certificate with an IP SAN for `127.0.0.1`, its matching private key, and the issuing CA certificate.
For a disposable local CA, follow [Server TLS](operations.md#server-tls), which produces `underwrap-pki/ca.pem`, `server.pem`, and `server.key`.
Run the following from that directory's parent, adjusting source paths if you used your own PKI.
Never put private keys in Nix expressions or copy them into the Nix store.

```bash
sudo install -d -m 0755 /etc/underwrap
sudo install -d -m 0750 -o root -g underwrap-backend /etc/underwrap-backend
sudo install -m 0644 underwrap-pki/ca.pem /etc/underwrap/ca.pem
sudo install -m 0644 underwrap-pki/server.pem /etc/underwrap-backend/server.pem
sudo install -m 0640 -o root -g underwrap-backend \
  underwrap-pki/server.key /etc/underwrap-backend/server.key
sudo install -d -m 0700 -o underwrap-backend -g underwrap-backend \
  /var/lib/underwrap-backend \
  /var/lib/underwrap-backend/stores \
  /var/lib/underwrap-backend/clients \
  /var/lib/underwrap-backend/users \
  /var/lib/underwrap-backend/data
```

Use a trusted editor to create `/etc/underwrap-backend/config.yaml` with the following non-secret settings.
Make it root-owned, group `underwrap-backend`, mode `0640`.

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

Trust `ca.pem` in the browser or its operating-system trust store before visiting this origin.
The daemon's CA file does not configure browser trust.
The CA signing key is not needed by the backend; keep it in separately controlled custody rather than beside backend data.

## 3. Bootstrap and start the backend

Keep the backend and every user-registry writer stopped.
Run the non-echoed [offline bootstrap procedure](operations.md#first-operator-bootstrap) with service identity `underwrap-backend`, config `/etc/underwrap-backend/config.yaml`, and username `alice`.
Its Python helper supplies exactly one password line on the CLI's stdin and refuses an echoing fallback; the backend CLI itself has no hidden-input prompt.
Do not enter a password at a bare backend command or put it in argv, environment variables, configuration, or shell history.

After bootstrap succeeds, start the backend in a dedicated terminal:

```bash
sudo -u underwrap-backend sh -c \
  'umask 077; exec underwrap-backend --config /etc/underwrap-backend/config.yaml'
```

Leave that process running for the exercise.
For persistent operation, replace the foreground launch with a reviewed [backend service deployment](operations.md#deploy-the-backend); this tutorial does not install one.

## 4. Create a store and enroll the frontend

Open `https://127.0.0.1:18443/` and sign in as the offline-provisioned operator.
In **Stores**, create a **plain** store named `demo`.
It persists plaintext, so use only a disposable demonstration value.
A runtime client instance must select an existing store.

On the frontend host, generate the identity:

```bash
sudo underwrap-keygen --instance-config /etc/underwrap/daemon/demo.yaml
```

A successful command installs a private combined identity at `/var/lib/underwrap-demo/client-demo.identity.pem` with mode `0400`, owned by `underwrap-demo`, and then prints only its public certificate.
Do not open or copy the private identity file.
If the command fails after installation, follow [identity failure handling](operations.md#runtime-client-identities) rather than assuming the previous identity remains installed.

In **Client instances**, create `demo-host`, select store `demo`, choose `Active`, paste the complete public certificate, and enable **Automatically create wrapper resources and pairs**.
Then restart the daemon; registration runs at startup:

```bash
sudo systemctl restart underwrapd-demo.service
```

Confirm resource `demo/token`, pair `secret-check → demo/token`, and a `Locked` permission row for `demo-host` now exist.
If auto-create is disabled, create the resource and pair manually and use the same restart and verification step.

## 5. Add the value and run the helper

In **Secrets**, select store `demo`, open `demo/token`, and add env entry `DEMO_TOKEN` with a nonempty non-production value.
In **Wrapper permissions**, select the exact `demo-host`, `secret-check`, `demo/token` permission row, choose `Unlocked`, leave **writable** off, and select **Save**.
Env is operator-owned and has no runtime edit path.

As Alice, run:

```bash
secret-check
```

Expected output:

```text
DEMO_TOKEN is available to the target
```

The explicit system-wide form is `/run/wrappers/bin/underwrap-demo secret-check`.
Do not use the debug shim for this check: debug deliberately grants a secret-disclosing target.
Presence-only success establishes this demonstration result, not comprehensive confidentiality or writeback durability.

## 6. Stop or retain the demonstration deliberately

Return the permission to `Locked` and save it when finished.
Stop the foreground backend in its terminal; encrypted production stores and browser sessions would also need attention after a restart.
The plain demonstration data remains on disk.
Use [Decommissioning](operations.md#decommissioning) if you want to remove the demonstration rather than retain it for later exercises.

If the command is missing, verify `allowedUsers` and try the explicit entry point.
For enrollment, TLS, missing-pair, or permission errors, follow [Connection and authorization diagnosis](operations.md#connection-and-authorization-diagnosis).
Never bypass the setgid entry point, disable TLS verification, or weaken host hardening to make the exercise pass.

Next, read [Wrappers](wrappers.md) before exposing a real capability, [Configuration](configuration.md) for options, and [Operations](operations.md) for encrypted stores and maintenance.
