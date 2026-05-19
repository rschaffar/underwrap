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
- **`underwrapd` ↔ `underwrap-backendd`**: HTTPS with mTLS. The client
  certificate is a self-signed leaf generated locally by the
  `underwrap-keygen` helper on the target host and registered with the
  backend manually by an admin pasting the cert PEM into the admin UI.
  The backend authenticates connections by SHA-256 fingerprint lookup
  against the runtime-client registry (no CA chain). The server cert is
  verified by the daemon against a pinned CA. The backend maps each
  authenticated client instance to one store and only serves that
  store's view.
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

https:
  listen: 127.0.0.1:18443
  tls:
    cert_file: /etc/underwrap-backend/server.pem
    key_file: /etc/underwrap-backend/server.key

admin:
  session_key_file: /run/keys/underwrap-admin-session-key
  # Directory the admin "create store" API places new dynamic stores
  # under. Each new store lands at `<stores_data_root>/<store_id>/`.
  # Unset disables store creation.
  stores_data_root: /var/lib/underwrap-backend/data
  # Where the admin API writes master-key files for encrypted dynamic
  # stores (one file per store, mode 0600). Defaults to
  # /var/lib/underwrap-backend/keys.
  keys_dir: /var/lib/underwrap-backend/keys
```

### Store registry

Stores are persisted in the backend-managed `stores.json` registry.
Each entry is a `StoreRecord` with a stable internal `id`, an
operator-facing `name`, optional `description`, optional `created_at`,
and the storage backend itself.

Example dynamic store record:

```json
{
  "version": 1,
  "stores": [
    {
      "id": "store_scratch",
      "name": "scratch",
      "description": "Ad-hoc operator scratchpad.",
      "created_at": 1729123456,
      "backend": "plain",
      "dir": "/var/lib/underwrap-backend/data/store_a1b2c3d4e5f6"
    }
  ]
}
```

`POST /v1/admin/stores` updates this registry atomically via
`tempfile::persist()`. Runtime client access is resolved dynamically
from the backend-managed client-instance registry. The presented client
certificate fingerprint must map to an active enrolled instance.
Operator login state is resolved dynamically from the backend-managed
operator-state file plus the session cookie.

## Runtime API

The HTTPS service exposes the small surface `underwrapd` needs, all over
`POST` except `GET /v1/status`:

- `GET /v1/status` — server liveness and the authenticated principal
- `POST /v1/read` — `{secret_id}` → `{material_b64 | null}`
- `POST /v1/read-prefixes` — one-shot bundle read for all leaves below a set of prefixes
- `POST /v1/write` — `{secret_id, material_b64}`
- `POST /v1/list` — `{prefix}` → `{ids}`
- `POST /v1/delete` — `{secret_id}` → `{existed}`
- `POST /v1/runtime/wrappers/register` — frontend auto-registration of
  `(wrapper, resource)` pairs. The backend reads the calling
  client-instance's `auto_create_wrapper_assets` flag to decide whether
  missing Resource and Wrapper/Resource Pair records may be created on
  this principal's behalf. Returns the list of newly-created entities,
  or a structured `AutoCreateDisabled` refusal listing what would have
  been created. The endpoint is upsert-only: existing entities are
  never modified, removals never happen, and `created_by` is set once
  on first sight.

Store selection is implicit in the authenticated principal. There is no
store name in the request body. Operators create runtime client
instances directly in the admin UI by pasting a cert PEM the helper
produced; the helper does not talk to the backend.

## Admin API

The admin API lives on the same HTTPS listener as the runtime API and
shares the same server certificate. Operator requests authenticate with
session cookies; state-changing requests must also satisfy the CSRF and
`allowed_origins` checks.

- `GET /v1/admin/client-instances` — list runtime client instances.
- `POST /v1/admin/client-instances` — register a new runtime client
  instance from an operator-pasted self-signed cert PEM. The backend
  stores the leaf's SHA-256 fingerprint and never consults the cert's
  subject, SAN, KU, EKU, or validity window — later mTLS connections
  are authorized purely by fingerprint match.
- `GET /v1/admin/client-instances/{id}`, `PUT`, `DELETE` — detail,
  update (including replacing `cert_pem` to rotate the credential), and
  delete.
- `GET /v1/admin/stores` — list of backend-managed stores
- `POST /v1/admin/stores` — create a store. For
  `backend = "encrypted"` the backend auto-generates a 32-byte master
  key, writes it to `<keys_dir>/<store>.key` with mode 0600, and
  returns the key value **exactly once** in the response as
  `key_hex`. Subsequent reads never include the key.
- `GET /v1/admin/stores/{name}`, `PUT`, `DELETE` — store detail /
  update / delete.
- `POST /v1/admin/stores/{name}/rotate-key` — generate a fresh
  master key for an encrypted store, replace the key file
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
- `POST /v1/admin/resources/{list,create,delete}` — manage the
  Resource records that own provenance for resource names. `delete`
  refuses with structured `blocked_by` counts when secrets, pairs, or
  grants still reference the resource.
- `POST /v1/admin/pairs/{list,create,delete}` — manage the
  Wrapper/Resource Pair records. `create` refuses if the referenced
  resource doesn't exist or if `(wrapper, resource)` is already taken;
  `delete` refuses when any grant references the pair.
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

## Resources and Wrapper/Resource Pairs

Resources and `(wrapper, resource)` pairs are first-class backend
entities, one persisted collection of each per store. Each record
carries write-once provenance:

```yaml
# __underwrap/resources/document
version: 1
resources:
  - name: github/token
    created_by: { kind: operator, id: op-1, username: alice }
    created_at: 1735689600
  - name: gws/state
    created_by: { kind: client-instance, id: inst_dev_laptop }
    created_at: 1735690500
```

```yaml
# __underwrap/pairs/document
version: 1
pairs:
  - wrapper: gh-authorized
    resource: github/token
    created_by: { kind: client-instance, id: inst_dev_laptop }
    created_at: 1735690500
```

Provenance is set on first creation and never changes afterwards.
There is no edit endpoint; `delete` on either entity refuses when
dependents exist (the response carries structured `blocked_by`
counts plus a small sample so the SPA can render a clear "fix this
first" message).

The `auto_create_wrapper_assets` flag on each client-instance record
gates whether `POST /v1/runtime/wrappers/register` may insert new
records on that principal's behalf. The flag defaults off; an admin
must flip it on per instance before its wrappers can register new
resources or pairs on startup.

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
