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
│   Unix-socket IPC)  │ mTLS │  - /v1/wrappers             │
│                     │      │  - /v1/wrapper-invocations  │
│                     │      │  - /v1/wrapper-invocations/ │
│                     │      │       finalize              │
│                     │      │  - /v1/admin/* + embedded UI│
└─────────────────────┘      └─────────────────────────────┘
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
# user, or runtime client exists.
stores_file: /var/lib/underwrap-backend/stores.json
client_instances_file: /var/lib/underwrap-backend/client-instances.json
users_file: /var/lib/underwrap-backend/users.json

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
```

Encrypted stores no longer have a `keys_dir` knob: the 32-byte master
key is generated in memory at store-creation time, displayed once in
the create-store response, and never written to disk. After a backend
restart every encrypted store boots locked until an operator submits
the key via `POST /v1/admin/stores/{id}/unlock`.

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

The HTTPS service exposes exactly three endpoints to `underwrapd`,
all `POST`. The wire speaks `(wrapper_name, resource_name)` only —
there is **no** way for a runtime client to address a backend
storage key (`__underwrap/*` or `<resource>/<slot>/<inner>`) directly.

### `POST /v1/wrappers` — register

```json
{ "wrappers": [
    { "wrapper_name": "gh-authorized", "resource_name": "github/token" }
] }
```

Frontend auto-registration of the daemon's `(wrapper_name,
resource_name)` pairs at startup. The backend reads the calling
client-instance's `auto_create_wrapper_assets` flag to decide whether
missing `Resource` and `Wrapper/Resource Pair` records may be created
on this principal's behalf. For every successfully-resolved pair the
backend also seeds a `Locked` permission for `(this client_instance,
wrapper_name, resource_id)` if none exists yet.

Response (tagged on `outcome`):

```json
{ "outcome": "ok",
  "auto_created_resources": ["github/token"],
  "auto_created_pairs": [{ "wrapper_name": "gh-authorized",
                            "resource_name": "github/token" }] }
```

or, when `auto_create_wrapper_assets` is `false` and at least one
pair is unknown:

```json
{ "outcome": "auto-create-disabled",
  "missing_resources": ["github/token"],
  "missing_pairs": [{ "wrapper_name": "gh-authorized",
                       "resource_name": "github/token" }] }
```

The endpoint is upsert-only: existing entities are never modified,
removals never happen, and `created_by` is set once on first sight.
Idempotent on repeat calls — re-registering an unchanged set returns
`outcome: ok` with empty creation lists.

### `POST /v1/wrapper-invocations` — open

```json
{ "wrapper_name": "gh-authorized", "resource_name": "github/token" }
```

The backend re-evaluates the policy server-side and replies with
either:

```json
{ "outcome": "allow",
  "resource_id": "res_a1b2c3…",
  "blob_b64": "…",
  "writable": false }
```

or a structured `Deny`:

```json
{ "outcome": "deny", "reason": "no-permission" }
```

Stable kebab-case reasons: `no-permission`, `locked` (or the
permission's free-text `reason` field verbatim, when set), `expired`,
`unregistered-pair`. The daemon caches `blob_b64` for the lifetime of
the wrapper-resource channel and serves the wrapper's `Checkout`
verbatim from it; it never tries to look up the storage key itself.

### `POST /v1/wrapper-invocations/finalize` — close

```json
{ "wrapper_name": "gh-authorized",
  "resource_name": "github/token",
  "blob_b64": "…or omitted for a usage-signal call…" }
```

The backend re-evaluates the policy again, then:

- `blob_b64 = None` — usage signal. Bumps `last_used_at` on the pair
  and returns `{ "outcome": "ok" }`. Always allowed if the open
  would have been allowed.
- `blob_b64 = Some(_)` — writeback. The backend re-checks the
  `writable` flag (deny `not-writable` if false), decodes the blob,
  re-canonicalizes it (deny `noncanonical-blob` if it doesn't match
  the submitted bytes), rejects unknown `schema_version` values, then
  enforces **env immutability**: the new blob's `env` section must
  byte-equal the stored blob's `env` section. Any change to `env`
  rejects with `InvalidBlob { reason: "env-section-immutable" }`.
- On a clean write the blob is atomically replaced; `last_used_at`
  is bumped on the pair regardless of which branch ran.

Tagged response variants:

```json
{ "outcome": "ok" }
{ "outcome": "deny", "reason": "not-writable" }
{ "outcome": "invalid-blob", "reason": "env-section-immutable" }
```

Structured `InvalidBlob` reasons: `decode-failed`, `noncanonical-blob`,
`schema-unknown`, `env-section-immutable`. They are split apart from
`Deny` because a `Deny` may flip to `Allow` after an operator edits
the policy, whereas `InvalidBlob` is a client bug that won't change
on retry.

Store selection is implicit in the authenticated principal. There is
no store name in any request body. Operators create runtime client
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
- `GET /v1/admin/stores` — list of backend-managed stores. Each item
  carries a `locked: bool` field; encrypted stores boot locked after
  every backend restart and stay locked until an admin unlocks them.
- `POST /v1/admin/stores` — create a store. For
  `backend = "encrypted"` the backend mints a fresh 32-byte master
  key in memory, writes a small unlock canary into the data
  directory (encrypted under that key), and returns the key value
  **exactly once** in the response as `key_hex`. Subsequent reads
  never include the key. The key never touches disk; operators are
  solely responsible for storing it.
- `GET /v1/admin/stores/{name}`, `PUT`, `DELETE` — store detail /
  update / delete. Locked stores can only be deleted with
  `?force=true` (the backend cannot inspect their contents to verify
  emptiness while locked).
- `POST /v1/admin/stores/{id}/unlock` — body `{key_hex}`. Decrypts
  the unlock canary with the supplied key to verify it; on success
  installs the backend in the registry's `Unlocked` state. Returns
  `200 {ok: true}`. Failure cases:
  - `400 invalid_key_hex` — value is not 64 hex characters.
  - `400 store_not_encrypted` — store is plain; nothing to unlock.
  - `403 key_mismatch` — key did not decrypt the canary; the
    registry stays locked.
  - `409 already_unlocked` — call is racing with another unlock.
- `POST /v1/admin/stores/{id}/lock` — drops the in-memory backend so
  subsequent runtime requests fail with `store_locked`. Idempotent:
  re-locking an already-locked store is a noop success.
- `POST /v1/admin/stores/{name}/rotate-key` — generate a fresh
  master key for an encrypted store, decrypt every resource under
  the current key and re-encrypt it under the new one, swap the
  data directory atomically, and return the new key value **exactly
  once**. Writes against the store are briefly serialized behind a
  per-store rotation mutex; reads continue throughout. Refuses with
  `503 store_locked` if the store is locked (rotation needs the
  current key to read existing ciphertext) and `409 not_encrypted`
  on plain stores.
- `POST /v1/admin/secret/{read,write,list,delete}` — direct access to any
  store's key-value surface. `write` and `delete` gate on
  [`writes_enabled`](#global-write-lock) and reject with
  `403 writes_globally_disabled` when the toggle is off.
- `POST /v1/admin/policy/get` — fetch the entire `PolicyDocument`
- `POST /v1/admin/policy/put` — replace the entire document
- `POST /v1/admin/wrapper-permissions/list` — list every wrapper
  permission in a store
- `POST /v1/admin/wrapper-permissions/upsert` — create or update one
  wrapper permission by `(client_instance, wrapper, resource)`
- `POST /v1/admin/wrapper-permissions/delete` — remove a wrapper
  permission by tuple
- `POST /v1/admin/resources/{list,create,delete}` — manage the
  Resource records that own provenance for resource names. `delete`
  refuses with structured `blocked_by` counts when secrets, pairs, or
  wrapper permissions still reference the resource.
- `POST /v1/admin/pairs/{list,create,delete}` — manage the
  Wrapper/Resource Pair records. `create` refuses if the referenced
  resource doesn't exist or if `(wrapper, resource)` is already taken;
  `delete` refuses when any wrapper permission references the pair.
- `POST /v1/admin/policy/writes-enabled/get` — read the per-store
  write toggle
- `POST /v1/admin/policy/writes-enabled/set` — flip the per-store
  write toggle. Admin-only; logged.

Every admin write is a full atomic replacement of the stored policy
document or of one wrapper-permission entry.

## Policy model

The backend persists one policy document per store under a reserved
secret id (`__underwrap/policy/document`).

### Wrapper permissions

A wrapper permission is one row keyed by the tuple
`(client_instance, wrapper, resource)`. The mode on the permission
decides whether the runtime client may use it *right now*:

```yaml
permissions:
  - client_instance: dev-laptop
    wrapper: gh-authorized
    resource: github/token
    mode:
      kind: unlocked
  - client_instance: dev-laptop
    wrapper: aws-prod
    resource: aws/prod
    mode:
      kind: locked
    reason: "ops approval required"
  - client_instance: ci
    wrapper: jira-readonly
    resource: jira/api
    mode:
      kind: unlocked-until
      until_epoch: 1745092800
```

Three modes:

- **`locked`** — deny every invocation through this permission. The
  free-text `reason` field is surfaced verbatim in the `Deny.reason`
  payload so operators can communicate intent without a roundtrip to
  Slack.
- **`unlocked`** — allow every invocation through this permission.
- **`unlocked-until`** — allow until `until_epoch`; deny after. The
  expiry is checked lazily on each request — there is no background
  sweeper.

Per-invocation approval is intentionally out of scope. If you need
human-in-the-loop confirmation, layer it at the OS level (sudo,
polkit) in front of the wrapper.

There is **no per-secret read filter**. Once the mode allows the
invocation, the daemon hands the wrapper every resource the wrapper's
declaration lists. To remove a runtime client's access to a resource,
edit the wrapper definition, not the policy.

Write authorization is decided per-permission via the `writable: bool`
flag and re-checked by the backend on every finalize. Read access
through an active permission is unconditional.

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

### Evaluation order (backend, on both open and finalize)

1. Resolve `resource_name` to a `Resource` record; if absent, deny
   `unregistered-pair`.
2. Look up the `(wrapper_name, resource_id)` pair; if absent, deny
   `unregistered-pair`.
3. Look up the wrapper permission for
   `(authenticated principal, wrapper_name, resource_id)`. No
   permission → deny `no-permission`.
4. Consult the permission's mode:
   - `locked` → deny with the operator-supplied `reason`, or
     `"locked"` if none was set.
   - `unlocked` → allow.
   - `unlocked-until { until_epoch }` → allow if
     `now <= until_epoch`, else deny `expired`.
5. On finalize with a writeback blob, additionally:
   - deny `not-writable` if the permission's `writable` flag is
     false (independent of the open's allow);
   - decode + re-canonicalize the bytes, reject
     `noncanonical-blob`, `decode-failed`, `schema-unknown` shapes;
   - reject `env-section-immutable` if the new blob's `env`
     section differs from the stored blob's `env` section.

The daemon plays no part in this — it forwards the call and relays
the outcome.

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
    last_used_at: 1735770100
```

Provenance is set on first creation and never changes afterwards.
There is no edit endpoint; `delete` on either entity refuses when
dependents exist (the response carries structured `blocked_by`
counts plus a small sample so the SPA can render a clear "fix this
first" message).

`last_used_at` (unix seconds, `None` until first use) is bumped
server-side on **both** the open and the finalize handler — i.e.
once at the start of a wrapper invocation and again at every
finalize (writeback or usage signal). The bump is idempotent at
one-second resolution and never blocks the request. Mount paths
are a daemon-local concern derived from the wrapper config; they
never travel on the wire and the pair record does not store them.

## Resource blob storage

Every resource is stored as **one** opaque key in the
`StorageBackend` at `resources/<resource_id>`. The value is the
JCS-canonical (RFC 8785) JSON bytes of a `ResourceBlob` carrying the
three slots inline:

```json
{
  "schema_version": 1,
  "env": { "GH_TOKEN": "<base64>" },
  "files": { "config.toml": "<base64>" },
  "libsecret": {
    "aliases": { "default": "default" },
    "collections": {
      "default": {
        "label": "Default", "created": 100, "modified": 200,
        "items": {
          "gh-cli": {
            "label": "gh CLI", "content_type": "text/plain",
            "created": 100, "modified": 200,
            "attributes": { "server": "github.com" },
            "secret": "<base64>"
          }
        }
      }
    }
  }
}
```

`BTreeMap` everywhere keeps key order deterministic; combined with
`serde_jcs` the canonical bytes are byte-identical for any two
logically-equal blobs. Change detection at the wrapper end-of-run
will be a byte comparison once the wrapper-side rewrite (story 410)
lands; today the daemon still re-serializes on every flush.

`PlainBackend` stores the blob as one file per resource;
`EncryptedBackend` AEAD-encrypts the blob as one ciphertext with one
nonce per resource. Operator-facing admin endpoints
(`/v1/admin/secret/{read,write,list,delete}`) keep the
`<resource>/<slot>/<inner>` address space but route through the blob
via a load-modify-save shim. **No runtime endpoint accepts a storage
key on the wire.**

The `auto_create_wrapper_assets` flag on each client-instance record
gates whether `POST /v1/wrappers` may insert new records on that
principal's behalf. The flag defaults off; an admin must flip it on
per instance before its wrappers can register new resources or pairs
on startup.

## What this replaces

The earlier design (SSH-forwarded Unix socket between `underwrapd` and
`underwrap-backendd`, local control socket, `underwrapctl`,
`underwrap-backendctl`, local timed-unlock flow) is gone. The codebase
supports exactly one shape — a single central HTTPS listener with
runtime client certificates for data and session-cookie auth for
administration. Documentation that
describes those retired components is historical only.
