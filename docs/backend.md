# The underwrap backend

underwrap splits its secret-handling responsibilities across two processes:

- **`underwrap-backendd`** — a central authenticated service that holds the
  real secret storage and the policy document, accessible over HTTPS.
- **`underwrapd`** — the local broker on the hardened NixOS machine. It
  authenticates wrappers from kernel identity and proxies authorized
  requests to the backend.

Operators manage the backend through the admin UI served directly by
`underwrap-backendd` on the same HTTPS listener as the runtime API.

## Architecture

```text
┌─────────────────────┐       ┌──────────────────────────┐
│  wrapped target     │       │  operator browser        │
│  (e.g. gh, jira)    │       │  (login + session cookie)│
└─────────┬───────────┘       └────────────┬─────────────┘
          │                                │
          │  wrapper socket (setgid,       │ HTTPS
          │  SO_PEERCRED verified)         │
          ▼                                ▼
┌─────────────────────┐      ┌─────────────────────────────┐
│  underwrapd         │ HTTPS│  underwrap-backendd         │
│  (per-instance,     ├─────▶│  one HTTPS listener         │
│   Unix-socket IPC)  │ mTLS │  - /v1/status, /v1/*        │
└─────────────────────┘      │  - /v1/admin/* + embedded UI│
                              └─────────────────────────────┘
```

## Trust boundaries

- **Wrapper ↔ `underwrapd`**: local, kernel-backed. The wrapper socket's
  `SO_PEERCRED` + effective GID (read from `/proc/<pid>/status`) select the
  wrapper name. Nothing the wrapper sends is trusted for identity.
- **`underwrapd` ↔ `underwrap-backendd`**: HTTPS with runtime client
  certificates issued by the backend's private client CA. The server cert is
  verified against a pinned CA. The backend maps each authenticated client
  instance to one store and only serves that store's view.
- **operator browser ↔ `underwrap-backendd`**: the same HTTPS listener,
  but without a client certificate. Admin routes authenticate via username +
  argon2id password hash + signed session cookie (HMAC-SHA256) + per-session
  CSRF token on every POST.

The wrapped target process is outside every trust boundary. It receives
only the secret material that the authorized read flow exposes.

## Backend configuration

`underwrap-backendd` takes a single YAML config. Store definitions
live in their own per-store YAML files under two directories, *not*
inline in this config (changed in ticket 230). All paths are absolute.

```yaml
# Optional — defaults kick in when omitted. Missing registry files are
# treated as empty, so the backend can boot before the first store,
# operator, or runtime client exists.
stores_file: /var/lib/underwrap-backend/stores.json
client_instances_file: /var/lib/underwrap-backend/client-instances.json
operators_file: /var/lib/underwrap-backend/operators.json

enrollment:
  invites_file: /var/lib/underwrap-backend/enrollment-invites.json
  # CA used to issue runtime client certificates from enrollment CSRs.
  # The runtime service's `client_ca_file` must trust this issuer.
  issuer_cert_file: /etc/underwrap-backend/client-ca.pem
  issuer_key_file: /run/keys/underwrap-backend-client-ca.key
  default_invite_ttl_secs: 86400

https:
  listen: 127.0.0.1:18443
  tls:
    cert_file: /etc/underwrap-backend/server.pem
    key_file: /etc/underwrap-backend/server.key
    client_ca_file: /etc/underwrap-backend/client-ca.pem

admin:
  session_key_file: /run/keys/underwrap-admin-session-key
  # Directory prefixes the admin "create store" API may place new
  # data dirs under. Empty disables store creation.
  stores_data_roots:
    - /var/lib/underwrap-backend/data
  # Where the admin API writes master-key files for encrypted dynamic
  # stores (one file per store, mode 0600). Defaults to
  # /var/lib/underwrap-backend/keys.
  keys_dir: /var/lib/underwrap-backend/keys
```

### Store definition files

Each store gets its own YAML under one of the two `stores_*_dir`
trees. Filename is `<store-name>.yaml`. The `<store-name>` must
match `[a-z0-9][a-z0-9_-]*`.

```yaml
# /etc/underwrap-backend/stores.d/work.yaml (declarative; owned by Nix)
backend: encrypted
dir: /var/lib/underwrap-backend/work
key_file: /run/keys/underwrap-work.key
description: Production work secrets.
```

```yaml
# /var/lib/underwrap-backend/stores.d/scratch.yaml (dynamic; admin API)
backend: plain
dir: /var/lib/underwrap-backend/data/scratch
description: Ad-hoc operator scratchpad.
created_at: 1729123456
```

- The **declarative** directory is owned by the deployment tool (the
  NixOS `services.underwrap-backend.declarativeStores` option writes
  symlinks into `/nix/store`). The admin API refuses to mutate
  declarative stores — edit the source tree instead.
- The **dynamic** directory is owned by `underwrap-backendd` itself.
  `POST /v1/admin/stores` writes here atomically via
  `tempfile::persist()`.
- Duplicate names across the two directories are a fatal startup
  error that prints both source paths.
- Runtime client access is resolved dynamically from the
  backend-managed client-instance registry. The presented client
  certificate fingerprint must map to an active enrolled instance.
- Operator login state is resolved dynamically from the
  backend-managed operator-state file plus the session cookie.

### NixOS module

The flake exposes `nixosModules.backend` with one option:

```nix
services.underwrap-backend.declarativeStores = {
  work = {
    backend = "encrypted";
    dir = "/var/lib/underwrap-backend/work";
    keyFile = "/run/keys/underwrap-work.key";   # sops-managed
    description = "Production work secrets.";
  };
  personal = {
    backend = "plain";
    dir = "/var/lib/underwrap-backend/personal";
  };
};
```

Each entry materialises as
`/etc/underwrap-backend/stores.d/<name>.yaml`, a symlink into the
Nix store. The module asserts at evaluation time that encrypted
stores supply a `keyFile`.

## Runtime API

The HTTPS service exposes the small surface `underwrapd` needs, all over
`POST` except `GET /v1/status`:

- `GET /v1/status` — server liveness and the authenticated principal
- `POST /v1/read` — `{secret_id}` → `{material_b64 | null}`
- `POST /v1/read-prefixes` — one-shot bundle read for all leaves below a set of prefixes
- `POST /v1/write` — `{secret_id, material_b64}`
- `POST /v1/list` — `{prefix}` → `{ids}`
- `POST /v1/delete` — `{secret_id}` → `{existed}`
- `POST /v1/catalog/put` — frontend auto-registration of `(host, user, wrapper, resource)` rows for UI use

Store selection is implicit in the authenticated principal. There is no
store name in the request body.

## Admin API

The admin API lives on the same HTTPS listener as the runtime API and
shares the same server certificate. Operator requests authenticate with
session cookies; state-changing requests must also satisfy the CSRF and
`allowed_origins` checks.

- `GET /v1/admin/enrollment-invites` — list backend-managed one-time
  enrollment invites. The raw invite secret is never returned from list
  responses or persisted to disk.
- `POST /v1/admin/enrollment-invites` — create an operator-approved
  enrollment invite for a `(hostname_hint, instance_name)` and target
  store. The response returns the invite secret exactly once plus an
  enrollment command that keeps the secret out of argv.
- `POST /v1/enroll` — unauthenticated CSR submission gated by invite id
  and one-time secret. The backend verifies CSR proof-of-possession but
  ignores CSR-requested subject/SAN/KU/EKU values; it signs a client-auth
  certificate with backend-controlled identity fields and creates or
  replaces the matching runtime client instance credential.
- `GET /v1/admin/stores` — list of configured stores
- `POST /v1/admin/stores` — create a dynamic store. For
  `backend = "encrypted"` the backend auto-generates a 32-byte master
  key, writes it to `<keys_dir>/<store>.key` with mode 0600, and
  returns the key value **exactly once** in the response as
  `key_hex`. Subsequent reads never include the key.
- `GET /v1/admin/stores/{name}`, `PUT`, `DELETE` — dynamic-store
  detail / update / delete. Mutations on declarative stores are
  refused with `409 declarative_immutable`.
- `POST /v1/admin/stores/{name}/rotate-key` — generate a fresh
  master key for an encrypted dynamic store, replace the key file
  atomically, reopen the backend, and return the new key value once.
  **Limitation:** v1 of rotation does not re-encrypt existing
  ciphertext, so secrets already stored under the store become
  unreadable through this backend. Re-encrypt them offline first if
  you need to keep them.
- `POST /v1/admin/secret/{read,write,list,delete}` — direct access to any
  store's key-value surface. `write` and `delete` gate on
  [`writes_enabled`](#global-write-lock) and reject with
  `403 writes_globally_disabled` when the toggle is off.
- `POST /v1/admin/policy/get` — fetch the entire `PolicyDocument`
- `POST /v1/admin/policy/put` — replace the entire document
- `POST /v1/admin/grants/list` — list every grant in a store
- `POST /v1/admin/grants/upsert` — create or update one grant by
  `(wrapper, host, user)`
- `POST /v1/admin/grants/delete` — remove a grant by tuple
- `POST /v1/admin/policy/writes-enabled/get` — read the per-store
  write toggle
- `POST /v1/admin/policy/writes-enabled/set` — flip the per-store
  write toggle. Admin-only; logged.

Every admin write is a full atomic replacement of the stored policy
document or of one grant entry.

## Policy model

The backend persists one policy document per store under a reserved
secret id (`__underwrap/policy/document`). After the ticket-245 rewrite
it has just two sections.

### Agent grants

A grant is one row keyed by the tuple `(wrapper, host, user)`. The mode
on the grant decides whether the agent can use that grant *right now*:

```yaml
grants:
  - wrapper: gh-authorized
    host: deepthought
    user: robert
    mode:
      kind: unlocked
  - wrapper: aws-prod
    host: deepthought
    user: robert
    mode:
      kind: per-call-approval
      approval_key: "ssh-ed25519 AAAA…"
    reason: "ops approval required"
  - wrapper: jira-readonly
    host: ci
    user: bot
    mode:
      kind: unlocked-until
      until_epoch: 1745092800
```

Four modes:

- **`locked`** — deny every invocation through this grant.
- **`unlocked`** — allow every invocation through this grant.
- **`unlocked-until`** — allow until `until_epoch`; deny after. The
  expiry is checked lazily on each request — there is no background
  sweeper.
- **`per-call-approval`** — every invocation must be accompanied by a
  fresh ssh-agent signature over a backend-issued nonce, verifiable
  against `approval_key` (OpenSSH public key format; ed25519 or RSA).

There is **no per-secret read filter**. Once the mode allows the
invocation, the daemon hands the wrapper every resource the wrapper's
declaration lists. To remove an agent's access to a resource, edit the
wrapper definition, not the policy.

### Global write lock

```yaml
writes_enabled: false
```

`writes_enabled` is a per-store kill switch over every write path in
the admin API (`/v1/admin/secret/{write,delete}`). When `false`, those
endpoints reject with `403 writes_globally_disabled`. Default `false` on
a fresh store — admins opt in deliberately before any mutation lands.
Wrapper-side writes through the libsecret channel inherit the same
gate via the `AccessMode::ReadWrite` projection.

### Evaluation order (daemon)

1. Look up the grant for `(wrapper, host, user)`. No grant → reject.
2. Consult the grant's mode:
   - `locked` → reject with the operator-supplied reason.
   - `unlocked` → proceed.
   - `unlocked-until { until_epoch }` → proceed if `now < until_epoch`,
     else reject.
   - `per-call-approval` → run the ssh-agent challenge round; on
     success, proceed.
3. Serve every resource the wrapper's declaration lists. The libsecret
   slot is projected `ReadOnly` if `writes_enabled = false`, `ReadWrite`
   otherwise.

## Per-call approval flow

```text
wrapper  ──Read───►  underwrapd  ──read policy──►  backend
wrapper  ◄─Challenge─  underwrapd  (evaluates, sees PerCallApproval)
wrapper  ──signs via ssh-agent──►  underwrapd
wrapper  ◄─ReadOk─────  underwrapd  (verified sig, fetched material)
```

The daemon keeps pending challenges in memory with a 60-second TTL. Each
nonce is single-use. Signatures are verified with
[`ssh-key`](https://docs.rs/ssh-key) against the `approval_key` stored in
the policy.

Central logging of the raw signature (sending the signed blob to the
backend in addition to local verification) is a later step; today the
backend sees only the resulting `secret.read` after the daemon accepts
the signature. The admin UI can still reason about "is this wrapper in
per-call mode?" because the mode lives in the policy document.

## What this replaces

The earlier design (SSH-forwarded Unix socket between `underwrapd` and
`underwrap-backendd`, local control socket, `underwrapctl`,
`underwrap-backendctl`, local timed-unlock flow) is gone. The codebase
supports exactly one shape — a single central HTTPS listener with
runtime client certificates for data and session-cookie auth for
administration. Documentation that
describes those retired components is historical only.
