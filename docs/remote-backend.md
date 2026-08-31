# Deploying the backend

A central deployment separates durable brokered-resource storage from the NixOS hosts that execute wrappers:

- **backend host** — runs `underwrap-backend`, stores resources and policy, and
  serves the admin UI;
- **frontend host** — runs one or more `underwrapd` instances and the NixOS
  wrapper chain;
- **operator browser** — connects to the backend's HTTPS origin.

The frontend makes outbound HTTPS connections only. Nix evaluation, daemon URL
admission, and the request client's HTTPS-only policy enforce this independently;
there is no plaintext, SSH-tunnel, or backend Unix-socket transport.

## Product and VM test topology

The product deployment described in this guide places backend and frontend roles on separate hosts connected by HTTPS.
The NixOS VM test does not reproduce that physical topology.
One NixOS VM runs both backend services and both frontend daemons.
Its local and remote-style frontend instances connect directly to `127.0.0.1:18443` and `127.0.0.1:18444` respectively.
The remote-style path exercises real backend-owned rustls, mTLS, certificate-fingerprint runtime principal handling, separate store and client registries, policy, resources, and backend service lifecycle.
The retained `tunnel` fixture identifiers name the second instance, service account, daemon, and test PKI, but they do not represent SSH transport.
The VM does not exercise DNS resolution, a non-loopback SAN or hostname, firewall traversal, cross-host routing, latency, partitions, distinct-host compromise, or multi-machine orchestration.

## Network and trust plan

Before installation, choose:

- the backend hostname and HTTPS port;
- a server CA trusted by every frontend and operator browser;
- the backend service account and state paths;
- plain or encrypted stores;
- which frontend runtime client instance maps to each store;
- whether each client may auto-create missing wrapper resources/pairs.

Expose the HTTPS port only to intended frontend hosts and operator networks.
TLS authentication does not replace ordinary firewalling and host hardening.

## Server TLS

underwrap uses a conventional server certificate for the backend endpoint. Its
SANs must include every hostname or IP used by frontends and browsers.

### HTTPS endpoint and proxy topology

The enforced URL is the endpoint `underwrapd` dials. Direct backend-owned rustls
HTTPS is supported. A URL naming an HTTPS nginx or other proxy frontend also
passes endpoint admission, and a Layer-4 TLS passthrough can preserve the
end-to-end mTLS connection.

Endpoint admission does not create generic Layer-7 proxy support. The backend
derives the runtime client instance from the peer certificate on its own rustls
connection. Ordinary TLS termination followed by plaintext HTTP neither reaches
that peer certificate boundary nor provides an accepted forwarded-certificate
or proxy-identity protocol.
underwrap supports no trusted proxy header, forwarded client certificate, or proxy service identity.
Do not configure an `http://` upstream as a workaround.

A minimal `step` recipe is:

```bash
step certificate create \
  "underwrap-backend-ca" ca.pem ca.key \
  --profile root-ca --insecure --no-password

step certificate create \
  "backend.example.internal" server.pem server.key \
  --profile leaf --ca ca.pem --ca-key ca.key \
  --san backend.example.internal \
  --san 127.0.0.1 \
  --not-after 8760h --insecure --no-password
```

Protect `ca.key` as a CA private key, and remove it from the backend host after issuance when the deployment does not require it there.

`server.key` and any retained CA signing key are operator-managed persistent authentication material outside resources and `StorageBackend`.
The backend reads the configured server key into rustls but does not install, rotate, atomically replace, or validate its ownership or mode.
The backend does not use or manage the CA signing key.
The operator owns key placement, filesystem ownership and mode, restrictive umask, rotation, service sandboxing, and CA-key custody.

Install:

- `server.pem` and `server.key` on the backend host;
- `ca.pem` on each frontend host and configure that instance's `backend.caFile`;
- CA trust in operator browsers or their operating systems.

For a private CA, `backend.caFile` is the daemon's complete, exclusive server
trust bundle; bundled WebPKI roots are not also trusted. For an ordinary
public-PKI server certificate, omit `backend.caFile` to use reqwest's bundled
WebPKI roots.

Runtime client certificates are not signed by this server CA. Each frontend
generates a self-signed leaf, and the backend uses its fingerprint after
operator enrollment to identify the runtime client instance.

## Backend host

`underwrap-backend` runs on Linux without the NixOS frontend module. A local
source build prepares the embedded SPA first:

```bash
cd web
npm ci
npm run build
cd ..
cargo build -p underwrap-backend
```

For a packaged deployment, install the `underwrap-backend` flake package; the
Nix build compiles and embeds the SPA automatically.

Create service-owned state directories and write
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
  allowed_origins:
    - https://backend.example.internal:18443
  stores_data_root: /var/lib/underwrap-backend/data
```

Run the process as a dedicated account that can read the TLS key and owns the registry/data directories.
These deployment controls supply TLS-key and state-path permissions that the application does not enforce.
A service unit should apply at least:

- `NoNewPrivileges=true`;
- `PrivateTmp=true`;
- `ProtectSystem=strict` with narrowly scoped writable state paths;
- `ProtectHome=true`;
- an empty capability set;
- restricted address families and syscall classes appropriate to an HTTPS
  service;
- `LimitCORE=0`;
- a restrictive umask.

The backend configuration and full data model are documented in
[Backend](backend.md). The annotated YAML is
[`examples/backend.yaml`](../examples/backend.yaml).

## Bootstrap the backend

Before the service exposes its listener, run the offline command as the backend
service identity:

```text
underwrap-backend --config /etc/underwrap-backend/config.yaml admin bootstrap-user --username <name>
```

Supply the password through standard input from a trusted prompt or password
manager. Do not put it in argv, environment variables, configuration, or shell
history. The command uses the service-owned user directory and bootstrap
sentinel, and refuses non-fresh state. Start the service, sign in through the
ordinary login page, then:

1. create the target store;
2. for an encrypted store, record the one-time master key in an external vault;
3. leave the encrypted store unlocked while enrolling and testing its frontend.

A runtime client instance must select an existing store, so store creation comes
before client enrollment.

## Recover total operator login loss

Perform total-login-loss recovery on the backend host under the deployed backend service identity and active configuration, not from a frontend host or browser.

Use the deployment service manager to stop the HTTPS listener and every other user-registry writer, then follow the normative [Offline operator recovery](backend.md#offline-operator-recovery) procedure before restarting the service.

## Frontend host

Configure the NixOS module with the remote URL and, for this private-CA
example, the exclusive server CA bundle:

```nix
services.underwrap = {
  enable = true;
  package = underwrap.packages.${pkgs.stdenv.hostPlatform.system}.default;

  instances.work = {
    allowedUsers = [ "alice" ];

    backend = {
      url = "https://backend.example.internal:18443";
      caFile = "/etc/underwrap/ca.pem";
    };

    wrappers.gh = {
      target = "${pkgs.gh}/bin/gh";
      resource = "github/gh";
      libsecret = true;
    };
  };
};
```

Rebuild the host. The daemon starts even when its runtime client credential does
not exist. Wrapper requests retry loading the credential and opening the backend
client, but wrapper-asset registration runs only at daemon startup. Restart the
daemon after enrollment, as shown below.

`allowedUsers` installs shims; it is not an ACL. Any local user can execute the
system-wide `underwrap-work` entry point. Add an OS-level gate when the host
requires local-user authorization.

## Generate the runtime client credential

On the frontend host:

```bash
sudo underwrap-keygen \
  --instance-config /etc/underwrap/daemon/work.yaml
```

The helper:

- generates a private key and self-signed client certificate locally;
- atomically installs one combined certificate-chain-and-key identity PEM at
  `/var/lib/underwrap-work/client-work.identity.pem` with mode `0400`;
- assigns it to the per-instance daemon user;
- prints only the public certificate PEM after the identity commit.

It does not contact the backend. Paste the certificate into **Client instances**
in the admin UI, choose the existing target store, and set the instance to
`Active`. The normative atomic-installation and failure boundaries are in
[Security](security.md#runtime-client-identity-installation).

For a non-NixOS frontend configuration tool, an explicit output path is also
available:

```bash
sudo underwrap-keygen \
  --client-identity-out /etc/underwrap/client-work.identity.pem
```

The frontend wrapper chain itself remains NixOS-supported; the explicit path is
useful for credential generation and development tooling.

## Register wrapper assets

Choose one of two enrollment policies.

### Automatic asset creation

Enable **Automatically create wrapper resources and pairs** on the client
instance, then restart the daemon:

```bash
sudo systemctl restart underwrapd-work.service
```

The startup registration creates missing resource records and wrapper/resource
pairs and seeds locked permission rows. Existing records are never modified or
removed by registration.

### Manual asset creation

Leave auto-create disabled and use the admin UI to create:

1. each resource named by frontend config;
2. each wrapper/resource pair.

Restarting the daemon reports any missing records but does not create them.
Manual creation gives operators explicit control over backend assets.

## Add resource values and permissions

For each wrapper:

1. add env/files/libsecret entries to its resource through **Secrets**;
2. open **Wrapper permissions**;
3. create or edit the permission row for the runtime client, wrapper, and
   resource;
4. choose the permission mode;
5. enable writable only when files/libsecret changes should persist.

Newly registered rows are locked. The canonical permission modes and evaluation
order are in [Backend](backend.md#policy-model). `UnsafeDebugUntil` is the only
mode that permits the debug target and should use a short deadline.

## Verify the deployment

On the frontend host:

```bash
systemctl status underwrapd-work.service
journalctl -u underwrapd-work.service
openssl s_client \
  -connect backend.example.internal:18443 \
  -servername backend.example.internal \
  -CAfile /etc/underwrap/ca.pem
```

Then run one narrow wrapper and confirm:

- the target succeeds;
- the admin UI updates `last_used_at` for its pair;
- the target runs under its dedicated `underwrap-target-*` UID;
- no secret value appears in daemon/backend logs;
- a read-only files/libsecret modification is discarded and audited;
- a writable modification persists only after finalize.

## Encrypted-store restart procedure

After every backend restart, encrypted stores are locked because their master
keys are not stored on disk.

1. Sign in to the admin UI.
2. Open **Stores**.
3. Unlock each required store with its externally held key.
4. Verify the store is unlocked before expecting wrapper invocations to work.

A locked store returns a structured runtime error, which the wrapper reports as
an operator-actionable message.

Rotation retains every displaced encrypted generation on the backend host. A
lost response or `rotation_recovery_required` error is handled while the remote
backend is stopped, not from a frontend host or browser. Follow the exact
non-overwriting [retained-generation recovery and selective-cleanup procedure](backend.md#retained-rotation-generations), then unlock and verify representative
data before resuming frontend traffic.

## Related documentation

- [Getting started](getting-started.md)
- [Admin UI](admin-ui.md)
- [Configuration](configuration.md)
- [Backend](backend.md)
- [Security](security.md)
