# The underwrap backend

underwrap splits its secret-handling responsibilities across two processes:

- **`underwrap-backendd`** — a central authenticated service that holds the
  real secret storage and the policy document, accessible over HTTPS.
- **`underwrapd`** — the local broker on the hardened NixOS machine. It
  authenticates wrappers from kernel identity and proxies authorized
  requests to the backend.

Operators manage the backend through the separate
[`underwrap-admin-ui`](admin-ui.md) service, which is a client of the
backend's admin HTTP API.

## Architecture

```text
┌─────────────────────┐       ┌──────────────────────────┐
│  wrapped target     │       │  operator browser        │
│  (e.g. gh, jira)    │       │  (login + session cookie) │
└─────────┬───────────┘       └────────────┬─────────────┘
          │                                │
          │  wrapper socket (setgid,       │  form POSTs
          │  SO_PEERCRED verified)         ▼
          ▼                     ┌────────────────────────┐
┌─────────────────────┐         │  underwrap-admin-ui    │
│  underwrapd         │         │  (plain HTTP over a    │
│  (per-instance,     │         │   TLS-terminating      │
│   Unix-socket IPC)  │         │   reverse proxy)        │
└─────────┬───────────┘         └────────────┬───────────┘
          │ mTLS                             │ Bearer token
          ▼                                  ▼
┌─────────────────────────────────────────────────────────┐
│  underwrap-backendd                                     │
│  ┌─────────────────────┐   ┌─────────────────────────┐  │
│  │ HTTPS runtime API   │   │ HTTP admin API          │  │
│  │ /v1/status, /v1/*   │   │ /v1/admin/*             │  │
│  └─────────────────────┘   └─────────────────────────┘  │
│  PolicyDocument + secret stores (plain | encrypted)     │
└─────────────────────────────────────────────────────────┘
```

## Trust boundaries

- **Wrapper ↔ `underwrapd`**: local, kernel-backed. The wrapper socket's
  `SO_PEERCRED` + effective GID (read from `/proc/<pid>/status`) select the
  wrapper name. Nothing the wrapper sends is trusted for identity.
- **`underwrapd` ↔ `underwrap-backendd`**: HTTPS with mutual TLS. The
  client certificate identifies an instance principal; the server cert is
  verified against a pinned CA. The backend maps each principal to one
  store and only serves that store's view.
- **admin UI ↔ `underwrap-backendd`**: HTTP with bearer tokens. Tokens are
  loaded from files on both sides (operator tokens on the backend,
  per-backend token on the UI) and compared in constant time.
- **operator ↔ admin UI**: username + argon2id password hash + signed
  session cookie (HMAC-SHA256) + per-session CSRF token on every POST.

The wrapped target process is outside every trust boundary. It receives
only the secret material that the authorized read flow exposes.

## Backend configuration

`underwrap-backendd` takes a single YAML config. All paths are absolute.

```yaml
stores:
  work:
    backend: plain
    dir: /var/lib/underwrap-backend/work
  personal:
    backend: encrypted
    dir: /var/lib/underwrap-backend/personal
    key_file: /run/keys/underwrap-personal.key

service:
  listen: 127.0.0.1:18443
  tls:
    cert_file: /etc/underwrap-backend/server.pem
    key_file: /etc/underwrap-backend/server.key
    client_ca_file: /etc/underwrap-backend/ca.pem
  clients:
    # Client principal (TLS subject CN) → which store it sees.
    underwrap-work@vm-01:
      store: work
    underwrap-personal@laptop-02:
      store: personal

admin:
  listen: 127.0.0.1:19443
  tokens:
    alice:
      token_file: /run/keys/underwrap-admin-alice
    bob:
      token_file: /run/keys/underwrap-admin-bob
```

- `stores`: one or more backing stores. Plain or encrypted (ChaCha20-Poly1305
  with a 32-byte key file).
- `service.clients`: maps each authenticated client principal (the TLS
  subject CN) to one store. Requests from other principals get 401; requests
  that would cross stores are impossible because the principal is bound to
  its store at authentication time.
- `admin.tokens`: opaque bearer tokens for operators. Each request to
  `/v1/admin/*` must carry an `Authorization: Bearer <token>` matching one
  of these (constant-time compared). The matched operator name is recorded
  in the audit log.

## Runtime API

The HTTPS service exposes the small surface `underwrapd` needs, all over
`POST` except `GET /v1/status`:

- `GET /v1/status` — server liveness and the authenticated principal
- `POST /v1/read` — `{secret_id}` → `{material_b64 | null}`
- `POST /v1/write` — `{secret_id, material_b64}`
- `POST /v1/list` — `{prefix}` → `{ids}`
- `POST /v1/delete` — `{secret_id}` → `{existed}`

Store selection is implicit in the authenticated principal. There is no
store name in the request body.

## Admin API

The admin listener is separate (HTTP, not HTTPS) and intended to sit
behind a local TLS-terminating reverse proxy when exposed beyond
localhost. Every request must present a bearer token. The operator name
tagged to the token ends up in audit events.

- `GET /v1/admin/stores` — list of configured store names
- `POST /v1/admin/secret/{read,write,list,delete}` — direct access to any
  store's key-value surface
- `POST /v1/admin/policy/get` — fetch the entire `PolicyDocument`
- `POST /v1/admin/policy/put` — replace the entire document
- `POST /v1/admin/wrapper/set-mode` — update one wrapper's approval mode
- `POST /v1/admin/rule/upsert`, `/rule/delete`
- `POST /v1/admin/grant/upsert`, `/grant/delete`

Every admin write is a full atomic replacement of the stored policy
document.

## Policy model

The backend persists one policy document per store under a reserved
secret id (`__underwrap/policy/document`). It has three sections.

### Wrappers — approval mode

Each wrapper entry carries its runtime mode:

```yaml
wrappers:
  - wrapper: gh-authorized
    mode:
      kind: unlocked
    # reason: optional operator note
  - wrapper: aws-prod
    mode:
      kind: per-call-approval
      approval_key: "ssh-ed25519 AAAA…"
    reason: "ops approval required"
  - wrapper: jira-readonly
    mode:
      kind: unlocked
      until_epoch: 1745092800
```

Modes:

- **`locked`** — deny every request.
- **`unlocked`** — allow requests subject to rules/grants. Optional
  `until_epoch`: past that time, the backend's sweeper transitions the
  wrapper back to `locked` automatically.
- **`per-call-approval`** — every request must be accompanied by a fresh
  SSH-agent signature over a backend-issued nonce, verifiable against
  `approval_key` (OpenSSH public key format; ed25519 or RSA).

Wrappers that are absent from the policy default to `unlocked` forever —
the backend only constrains the wrappers it has been told about.

### Rules and grants

Rules and grants share the authorization tuple:

```text
(wrapper, resource, operation) → allow | deny
```

- **Rules** are permanent.
- **Grants** layer on top with `starts_at_epoch` / `expires_at_epoch`.

Resources are strings. Underwrap uses the prefix convention
`<kind>:<id>`:

- `secret:github/token` — an env-injected secret
- `libsecret:gh/default` — a libsecret namespace
- `directory:gws/state` — a directory-credential tree

Operations mirror: `secret.read`, `libsecret.read|write|readwrite`,
`directory.read|write|readwrite`.

### Evaluation order

1. If the wrapper is `locked` → deny with the configured reason.
2. If `unlocked { until_epoch }` has expired → deny.
3. If `per-call-approval` → surface `NeedsApproval{approval_key}`; the
   daemon runs the challenge round-trip and re-evaluates with the mode
   gate bypassed.
4. Otherwise: a matching active deny rule/grant → deny.
5. A matching active allow grant or rule → allow.
6. No match → deny.

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

Central audit of the raw signature (sending the signed blob to the
backend in addition to local verification) is a later step; today the
backend sees only the resulting `secret.read` after the daemon accepts
the signature. The admin UI can still reason about "is this wrapper in
per-call mode?" because the mode lives in the policy document.

## Expiry sweeper

`underwrap-backendd` spawns a background task every 30 seconds that walks
each configured store's policy document, finds every
`Unlocked { until_epoch: Some(t) }` with `now > t`, and flips the entry
to `Locked` with an operator-visible `reason` explaining the automatic
transition. This keeps the admin UI's view and the real policy state in
agreement without relying on a read request to land.

## What this replaces

The earlier design (SSH-forwarded Unix socket between `underwrapd` and
`underwrap-backendd`, local control socket, `underwrapctl`,
`underwrap-backendctl`, local timed-unlock flow) is gone. The codebase
supports exactly one shape — central HTTPS service with mTLS for data
and HTTP with bearer tokens for administration. Documentation that
describes those retired components is historical only.
