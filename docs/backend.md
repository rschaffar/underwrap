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
          │  wrapper socket (0660,         │ HTTPS
          │  egid via SO_PEERPIDFD)        │
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

- **Wrapper ↔ `underwrapd`**: local, kernel-backed. The peer's effective
  GID gates the `0660` socket and is read race-free via `SO_PEERPIDFD`
  (Linux 6.5+; `SO_PEERCRED` + `/proc/<pid>/status` fallback on older
  kernels). The client-supplied wrapper name is only a selector, validated
  against config — nothing the wrapper sends is trusted for identity.
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
  argon2id password hash + an opaque `tower-sessions` session cookie (session
  state held server-side in a `MemoryStore`) + a synchronizer-token CSRF
  check on every POST.

The wrapped target process is outside every trust boundary. It receives
only the secret material that the authorized read flow exposes.

## Backend configuration

`underwrap-backendd` takes a single YAML config. Store definitions
live in the backend-managed store registry (`stores_dir`), *not*
inline in this config. All paths are absolute.

```yaml
# Optional — defaults kick in when omitted. Missing registry files are
# treated as empty, so the backend can boot before the first store,
# user, or runtime client exists.
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

Stores are persisted as one JSON file per record under
`stores_dir` (default `/var/lib/underwrap-backend/stores`). The id
is the file stem (so `stores/work.json` is the `work` store) and is
not duplicated inside the body. Each file is a `StoreRecord` carrying
the operator-facing `name`, optional `description`, optional
`created_at`, and the storage backend itself.

Example dynamic store record at `stores/store_scratch.json` — one file
per store, no wrapper array, and no `id` inside the body (the id is the
file stem):

```json
{
  "name": "scratch",
  "description": "Ad-hoc operator scratchpad.",
  "created_at": 1729123456,
  "backend": "plain",
  "dir": "/var/lib/underwrap-backend/data/store_a1b2c3d4e5f6"
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
storage key (`control/*` or `<resource>/<slot>/<inner>`) directly.

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
{ "wrapper_name": "gh-authorized",
  "resource_name": "github/token",
  "variant": "real" }
```

`variant` is `"real"` (default, omittable) or `"debug"`. It is the
invocation flavour the daemon relays from
`uw-<instance> <wrapper>` vs `uw-<instance> debug <wrapper>`. The
backend evaluates the policy for the variant; only an active
`UnsafeDebugUntil` permission admits `"debug"` (see
[Policy model](#policy-model)). The `variant` is trustworthy
end-to-end because the **daemon**, not the caller, then picks the target
binary the wrapper execs — a forged `"real"` cannot reach the debug
tool.

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
`not-unsafe-debug` (a `"debug"` variant against a non-`UnsafeDebugUntil`
permission), `unregistered-pair`. The daemon caches `blob_b64` for the
lifetime of the wrapper-resource channel and serves the wrapper's
`Checkout` verbatim from it; it never tries to look up the storage key
itself.

### `POST /v1/wrapper-invocations/finalize` — close

```json
{ "wrapper_name": "gh-authorized",
  "resource_name": "github/token",
  "variant": "real",
  "blob_b64": "…or omitted for a usage-signal call…" }
```

The backend re-evaluates the policy again (same `variant` matrix as
open), then:

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

The complete admin API contract is described by the OpenAPI 3.1 document
at `crates/backend-backendd/openapi.json`, derived from
`#[utoipa::path(...)]` annotations on each handler plus
`#[derive(utoipa::ToSchema)]` on each DTO. The mTLS runtime API
(`/v1/wrappers`, `/v1/wrapper-invocations*`) is deliberately *not* in
the spec — it is an internal daemon-to-backend contract, not a
front-end consumable.

Regenerate the spec by running `cargo test --workspace`; the
`openapi_export.rs` drift test rewrites `openapi.json` in place and
fails until the file is committed.

The SPA's API client (`web/src/api/client.ts`, built on
`openapi-fetch`) consumes the same spec: `npm run generate:api`
re-runs `openapi-typescript` against `openapi.json` to produce the
ignored local file `web/src/api/schema.ts`. The `predev`, `prebuild`,
and `pretest` npm hooks fire this automatically, so a stale spec
breaks the SPA build at the earliest possible point. There is no
separate ts-rs binding step — the OpenAPI schema is the single source
of typed truth.

Two SecuritySchemes are referenced from individual operations:
`session_cookie` (the opaque `__Host-uw-session` cookie issued by
tower-sessions) and `csrf_header` (the synchronizer `X-CSRF-Token`
header issued by `axum_csrf`). Public endpoints (login, whoami,
bootstrap) carry neither; read-only protected endpoints carry only
`session_cookie`; state-changing protected endpoints carry both.


The full route table follows, grouped by resource and using the real
HTTP verbs. `openapi.json` (and the handler annotations it is generated
from) is the source of truth; this list is a human-readable index. Path
parameters: `{id}`/`{sid}` is a store, `{rid}` a resource, `{cid}` a
libsecret collection, `{iid}` a libsecret item.

### Authentication and session

- `GET  /v1/admin/auth/bootstrap/status` — *public*; reports whether the
  first operator account still needs to be created.
- `POST /v1/admin/auth/bootstrap` — *public*; creates the very first
  operator account (only succeeds while the user collection is empty).
- `POST /v1/admin/auth/login` — *public*; establishes the session cookie.
- `POST /v1/admin/auth/logout` — end the current session.
- `GET  /v1/admin/auth/whoami` — current principal.
- `POST /v1/admin/auth/password` — self-service password change.

### Users (operator login accounts)

- `GET    /v1/admin/users` — list.
- `POST   /v1/admin/users` — create.
- `PUT    /v1/admin/users/{id}` — rename / update.
- `DELETE /v1/admin/users/{id}` — delete (a "last user" guard keeps the
  collection non-empty).
- `POST   /v1/admin/users/{id}/password` — admin reset of another user's
  password.

### Runtime client instances

- `GET    /v1/admin/client-instances` — list.
- `POST   /v1/admin/client-instances` — register from an operator-pasted
  self-signed cert PEM. The backend stores the leaf's SHA-256
  fingerprint and never consults the cert's subject, SAN, KU, EKU, or
  validity window — later mTLS connections are authorized purely by
  fingerprint match.
- `PUT    /v1/admin/client-instances/{id}` — update (including replacing
  `cert_pem` to rotate the credential).
- `DELETE /v1/admin/client-instances/{id}` — delete.

### Stores

- `GET    /v1/admin/stores` — list. Each item carries `locked: bool`;
  encrypted stores boot locked after every restart and stay locked until
  an admin unlocks them.
- `POST   /v1/admin/stores` — create. For `backend = "encrypted"` the
  backend mints a fresh 32-byte master key in memory, writes an
  encrypted unlock canary, and returns the key value **exactly once** as
  `key_hex`. The key never touches disk; operators are solely
  responsible for storing it.
- `PUT    /v1/admin/stores/{id}` — update.
- `DELETE /v1/admin/stores/{id}` — delete. Locked stores require
  `?force=true` (contents can't be verified empty while locked).
- `POST   /v1/admin/stores/{id}/unlock` — body `{key_hex}`; decrypts the
  canary to verify the key, then installs the `Unlocked` backend.
  Failure cases: `400 invalid_key_hex`, `400 store_not_encrypted`,
  `403 key_mismatch`, `409 already_unlocked`.
- `POST   /v1/admin/stores/{id}/lock` — drop the in-memory backend so
  subsequent runtime requests fail `store_locked`. Idempotent.
- `POST   /v1/admin/stores/{id}/rotate-key` — re-encrypt every resource
  under a fresh master key, swap the data directory atomically, and
  return the new `key_hex` **exactly once**. Writes serialize behind a
  per-store rotation mutex; reads continue throughout. `503 store_locked`
  if locked, `409 not_encrypted` on plain stores.

### Resources

First-class `ResourceRecord`s that own name provenance.

- `GET    /v1/admin/stores/{id}/resources` — list.
- `POST   /v1/admin/stores/{id}/resources` — create.
- `PUT    /v1/admin/stores/{id}/resources/{rid}` — update.
- `DELETE /v1/admin/stores/{id}/resources/{rid}` — delete; refuses with
  structured `blocked_by` counts when secrets, pairs, or wrapper
  permissions still reference the resource.

### Wrapper/Resource pairs

- `GET    /v1/admin/stores/{id}/pairs` — list.
- `POST   /v1/admin/stores/{id}/pairs` — create against an existing
  resource; refuses if the resource doesn't exist or `(wrapper,
  resource)` is already taken.
- `POST   /v1/admin/stores/{id}/pairs/with-new-resource` — create a pair
  and its resource in one atomic call.
- `DELETE /v1/admin/stores/{id}/pairs/{wrapper}/{rid}` — delete; refuses
  while any wrapper permission references the pair.

### Secrets (per-slot entries inside a resource blob)

Direct access to a store's per-secret surface, addressed by resource id
+ slot + key. Admin writes are unconditional — there is no global write
toggle (see [No global write lock](#no-global-write-lock)).

- `GET    /v1/admin/stores/{id}/secrets` — list entries (keys, never
  values).
- `GET    /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}` — read one
  entry.
- `PUT    /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}` — create or
  overwrite one entry.
- `DELETE /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}` — delete.
- `POST   /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}/rename` —
  atomically rename one entry.

### Libsecret slot (Secret Service data model)

Scoped under `/v1/admin/stores/{sid}/resources/{rid}/libsecret`:

- `GET    …/libsecret` — full collection/item tree for the slot.
- `POST   …/libsecret/collections` — create a collection.
- `DELETE …/libsecret/collections/{cid}` — delete a collection.
- `POST   …/libsecret/collections/{cid}/items` — create an item.
- `DELETE …/libsecret/collections/{cid}/items/{iid}` — delete an item.
- `GET    …/libsecret/collections/{cid}/items/{iid}/secret` — reveal one
  item's secret value.
- `PUT    …/libsecret/aliases/{name}` / `DELETE …/libsecret/aliases/{name}`
  — set or clear a collection alias (e.g. `default`).

### Wrapper permissions (the policy surface)

- `GET    /v1/admin/stores/{id}/wrapper-permissions` — list every
  permission row in the store.
- `PUT    /v1/admin/stores/{id}/wrapper-permissions/{client_instance}/{wrapper}/{rid}`
  — create or update one permission by
  `(client_instance, wrapper, resource)` tuple.
- `DELETE /v1/admin/stores/{id}/wrapper-permissions/{client_instance}/{wrapper}/{rid}`
  — remove one permission by tuple.

### Denormalized views (SPA convenience reads)

- `GET /v1/admin/stores/{id}/secrets-view` — secrets joined with their
  resource/pair provenance.
- `GET /v1/admin/stores/{id}/wrapper-permissions-view` — permissions
  joined with resolved wrapper / resource / client-instance names.

Every admin write is an atomic replacement of the affected document or of
the single addressed entry.

## Policy model

The backend persists one policy document per store under a reserved
control-plane key (`control/policy`).

### Wrapper permissions

A wrapper permission is one row keyed by the tuple
`(client_instance, wrapper, resource)`. The mode on the permission
decides whether the runtime client may use it *right now*:

```yaml
permissions:
  - client_instance: dev-laptop
    wrapper: gh-authorized
    resource_id: res_a1b2c3d4e5f6
    mode:
      kind: unlocked
    writable: false
  - client_instance: dev-laptop
    wrapper: aws-prod
    resource_id: res_aaa222bbb333
    mode:
      kind: locked
    writable: false
    reason: "ops approval required"
  - client_instance: ci
    wrapper: jira-readonly
    resource_id: res_ccc444ddd555
    mode:
      kind: unlocked-until
      until_epoch: 1745092800
    writable: false
```

`resource_id` is the stable `res_<hex>` from the resources document, not the
operator-facing name; renaming a resource therefore does not invalidate any
permission. The tuple `(client_instance, wrapper, resource_id)` is the
unique key.

Four modes:

- **`locked`** — deny every invocation through this permission. The
  free-text `reason` field is surfaced verbatim in the `Deny.reason`
  payload so operators can communicate intent without a roundtrip to
  Slack.
- **`unlocked`** — allow the `real` variant; deny `debug`.
- **`unlocked-until`** — like `unlocked` until `until_epoch`; deny
  after. The expiry is checked lazily on each request — there is no
  background sweeper.
- **`unsafe-debug-until`** — allow **both** the `real` wrapper and its
  `debug` variant until `until_epoch`. The only mode under which the
  debug tool runs; it gives the operator an env summary, an interactive
  shell, or an exec inside the wrapped namespace with **read-write**
  access to the projected secret (a `debug` run can write the secret
  back on the same terms as `real`, gated only by `writable`), so use
  the shortest TTL that fits.

The `(mode × variant)` matrix (evaluated by the shared `decide`):

| mode | `real` | `debug` |
|------|--------|---------|
| `locked` | deny (`locked`/reason) | deny (`locked`/reason) |
| `unlocked` | allow | deny (`not-unsafe-debug`) |
| `unlocked-until` active | allow | deny (`not-unsafe-debug`) |
| `unlocked-until` expired | deny (`expired`) | deny (`expired`) |
| `unsafe-debug-until` active | allow | allow |
| `unsafe-debug-until` expired | deny (`expired`) | deny (`expired`) |

Per-invocation approval is intentionally out of scope. If you need
human-in-the-loop confirmation, layer it at the OS level (sudo,
polkit) in front of the wrapper.

There is **no per-secret read filter**. Once the mode allows the
invocation, the daemon hands the wrapper the resource the wrapper
declares (one per wrapper). To remove a runtime client's access to a
resource, either change the wrapper's `resource` to point elsewhere
or delete the permission.

Write authorization is decided per-permission via the `writable: bool`
flag and re-checked by the backend on every finalize. Read access
through an active permission is unconditional.

### No global write lock

There is no per-store write toggle. Admin secret edits are
unconditional for an authenticated admin; wrapper-side writes are gated
solely by the per-permission `writable` flag, re-checked on every
finalize.

### Evaluation order (backend, on both open and finalize)

1. Resolve `resource_name` to a `Resource` record; if absent, deny
   `unregistered-pair`.
2. Look up the `(wrapper_name, resource_id)` pair; if absent, deny
   `unregistered-pair`.
3. Look up the wrapper permission for
   `(authenticated principal, wrapper_name, resource_id)`. No
   permission → deny `no-permission`.
4. Consult the permission's mode + the request's `variant` via the
   shared `policy::decide` matrix:
   - `locked` → deny with the operator-supplied `reason`, or
     `"locked"` if none was set (both variants).
   - `unlocked` → allow `real`, deny `debug` with `not-unsafe-debug`.
   - `unlocked-until { until_epoch }` not expired → allow `real`,
     deny `debug` with `not-unsafe-debug`; expired → deny `expired`.
   - `unsafe-debug-until { until_epoch }` not expired → allow both
     `real` and `debug`; expired → deny `expired`.
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
# control/resources
resources:
  - id: res_a1b2c3d4e5f6
    name: github/token
    created_by: { kind: operator, id: op-1, username: alice }
    created_at: 1735689600
  - id: res_0123456789ab
    name: gws/state
    created_by: { kind: client-instance, id: inst_dev_laptop }
    created_at: 1735690500
```

```yaml
# control/pairs
pairs:
  - wrapper: gh-authorized
    resource_id: res_a1b2c3d4e5f6
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
  "files": { ".config/gh/hosts.yml": "<base64>" },
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
will be a byte comparison once the wrapper-side rewrite lands;
today the daemon still re-serializes on every flush.

Keys in the `files` section are full relative paths inside the
target's home view (e.g. `.config/gh/hosts.yml`). The backend treats
them as opaque strings — it only enforces the usual no-traversal /
no-leading-slash hygiene — but the frontend wrapper's `files.paths`
determines which keys are projectable. The first path component(s)
of every key must match one of the wrapper's configured mount paths;
ns aborts the invocation if any blob key has no matching mount.

`PlainBackend` stores the blob as one file per resource;
`EncryptedBackend` AEAD-encrypts the blob as one ciphertext with one
nonce per resource. The operator-facing per-secret admin endpoints
(`/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}`) keep the
`<resource>/<slot>/<key>` address space but route through the blob
via a load-modify-save shim. **No runtime endpoint accepts a storage
key on the wire.**

Alongside the resource blobs, each store holds a small reserved
**control-plane** namespace. Resource ids are server-generated `res_*`
tokens, so user blobs always sit under `resources/`; everything that is
*not* user secret material lives under a single sibling `control/`
prefix:

```text
<store>/
  resources/<res_id>     # one JCS resource blob per resource (the secrets)
  control/policy         # PolicyDocument   (wrapper-permission rows)
  control/resources      # ResourcesDocument (resource registry + provenance)
  control/pairs          # PairsDocument    ((wrapper, resource) + last_used_at)
  control/canary         # encrypted stores only: unlock canary
```

A key is internal iff it starts with `control/`
(`underwrap_protocol::control::is_control_key`) — the one rule behind
the secret browser's filtering, the "has secrets" probe, and the
delete-empty guard.

The `auto_create_wrapper_assets` flag on each client-instance record
gates whether `POST /v1/wrappers` may insert new records on that
principal's behalf. The flag defaults off; an admin must flip it on
per instance before its wrappers can register new resources or pairs
on startup.

