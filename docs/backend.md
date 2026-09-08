# The underwrap backend

underwrap splits its secret-handling responsibilities across two processes:

- **`underwrap-backend`** — a central authenticated service that holds the
  real secret storage and the policy document, accessible over HTTPS.
- **`underwrapd`** — the local broker on the hardened NixOS machine. It
  authenticates wrappers from kernel identity and proxies authorized
  requests to the backend.

Operators manage the backend through the admin UI served directly by
`underwrap-backend` on the same HTTPS listener as the runtime API.

## Contents

- [Architecture](#architecture)
- [Backend configuration](#backend-configuration)
- [Offline operator recovery](#offline-operator-recovery)
- [Runtime API](#runtime-api)
- [Admin API](#admin-api)
  - [Backend migration recovery](#backend-migration-recovery)
- [Policy model](#policy-model)
- [Resources and wrapper/resource pairs](#resources-and-wrapperresource-pairs)
- [Resource blob storage](#resource-blob-storage)

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
│  underwrapd         │ HTTPS│  underwrap-backend          │
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
- **`underwrapd` ↔ `underwrap-backend`**: HTTPS with mTLS. The client
  certificate is a self-signed leaf generated locally by the
  `underwrap-keygen` helper on the target host and registered with the
  backend manually by an admin pasting the cert PEM into the admin UI.
  The backend authenticates connections by SHA-256 fingerprint lookup
  against the runtime-client registry (no CA chain). Separately, the daemon
  authenticates the server certificate against its configured CA bundle as the
  complete server trust set, or against reqwest's bundled WebPKI roots when no
  bundle is configured. The backend maps each authenticated client instance to
  one store and only serves that store's view. Authentication selects the
  principal and store; runtime JSON bodies remain parser-untrusted input and
  are validated before use.
- **operator browser ↔ `underwrap-backend`**: the same HTTPS listener,
  but without a client certificate. Admin routes authenticate via username +
  Argon2id password hash + an opaque `tower-sessions` session cookie (session
  state held server-side in a `MemoryStore`) + a synchronizer-token CSRF
  check on every state-changing request.

The configured target is the authorized recipient of the selected projection.
It is trusted not to disclose that material; underwrap does not sandbox a
malicious target.

## Backend configuration

`underwrap-backend` takes a single YAML config. Store definitions
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

Before encrypted creation or plain-to-encrypted migration, the authenticated admin key endpoint generates a fresh 32-byte master key in memory and returns it without mutating storage.
The operator must submit that exact untrimmed 64-character lowercase hexadecimal key to the later create or migration request.
Encrypted-to-plain migration generates and accepts no key.
Underwrap never writes an encrypted-store master key to disk, and create or migration success does not return the key again.
Key rotation retains its separate one-time post-mutation key response.
After a backend restart, every encrypted store starts locked until an operator submits its key via `POST /v1/admin/stores/{id}/unlock`.

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

The store list projects `ready`, `unlocked`, `locked`, `busy`, or `recovery-required` independently of the configured `plain` or `encrypted` backend kind.
Its action fields are authoritative for edit, migration, rotation, lock, unlock, and usage-dependent deletion controls.
Locked, busy, and recovery-required stores expose conservative unknown-content usage rather than claiming inspected emptiness.

Store records and runtime client instance records use one write lock per live
registry. A same-registry create, update, or delete becomes observable to other
requests only after its registry-file save or delete helper reports success;
the successful mutation is published before that registry write lock is
released. If the helper returns an error, the request returns its existing 500,
the running live authority remains at the exact pre-request record set, and a
fresh load observes the old persisted state once the registry path is restored
and readable. Writes sync the temporary file and use `tempfile::persist()` for
atomic replacement visibility. They do not fsync the parent directory; deletes
are ordinary unlinks, so neither path claims power-loss durability.

Runtime client access is resolved dynamically from the backend-managed
client-instance registry. The presented client certificate fingerprint must map
to an active enrolled instance. Runtime client remap/delete permission cleanup
runs only after the registry commit and lock release and remains best-effort: a
cleanup warning does not roll back or fail an already committed registry
mutation. Operator identity is resolved from the per-user registry under
`users_dir` and the server-side in-memory session addressed by the opaque
cookie.

This is a same-registry running-process guarantee, not a transaction or global
lock. Store deletion checks client mappings and backing-store contents before
its short registry commit window, and runtime client updates consult the store
registry separately; concurrent cross-registry or backing-store changes can
invalidate those check/use results.
There is no revision, compare-and-swap, automatic retry, reconciliation, or repair.
Store creation also creates its backing data directory—and an encrypted canary for an encrypted store—before the store-registry commit; a failed registry save can leave those pre-registry side effects for trusted-host cleanup.
Conditional key validation completes before any create-side filesystem mutation.

## Offline operator recovery

Use this procedure only for total login loss, when no operator can authenticate through the ordinary HTTPS login flow.

`recover-user` is an offline backend-host operation, not a network API or browser recovery path.

1. Identify the active backend configuration path and the Unix service identity used by the deployed backend.
2. Stop the backend service so its HTTPS listener is down, and stop every other process that can write the configured user registry, including other offline administrative commands.
3. Choose a distinct new username after accounting for normalization, which trims surrounding whitespace and lowercases the name.
4. As the same backend service identity, run the following command with the active configuration and with exactly one password line connected to stdin from a trusted prompt or password-manager pipe:

   ```text
   underwrap-backend --config <active-config-path> admin recover-user --username <distinct-new-name>
   ```

5. Require the command to report success before restarting the backend service.
6. Restart the backend, sign in through the ordinary login page as the recovery user, and use **Users** to create, rename, or reset the intended operator account as needed.
7. Sign out and verify that another repaired or newly created operator account can sign in successfully.
8. Optionally delete the recovery user through **Users** only after another working operator account has been verified.

Never place the password value in argv, environment variables, configuration, shell history, logs, or this procedure.

The plaintext password exists transiently in recovery process memory, and the Argon2id hash is the only password representation persisted in the new per-user record.

The command loads the configured registry and refuses an existing normalized username rather than resetting or overwriting that account.

The command neither reads nor changes the bootstrap sentinel, so recovery does not reopen first-use bootstrap.

> [!WARNING]
> Do not delete the bootstrap sentinel or the user registry as a recovery shortcut.
>
> Do not generate, replace, or edit password hashes manually.
>
> These shortcuts can destroy registry integrity or first-use state instead of restoring controlled operator access.

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
removals never happen, and `created_by` is set once on first sight. Empty or
whitespace-only names are invalid. Repeating the same exact Rust-string
`(wrapper_name, resource_name)` pair within one request returns HTTP 400 with
code `duplicate_wrapper_resource_pair` before the auto-create setting or store
is read; names are not trimmed, normalized, or case-folded for this check.
Rows that share only a wrapper or only a resource remain valid. Registration is
idempotent across separate calls — re-registering an unchanged pair in a later
request returns `outcome: ok` with empty creation lists.

### `POST /v1/wrapper-invocations` — open

```json
{ "wrapper_name": "gh-authorized",
  "resource_name": "github/token",
  "variant": "real" }
```

`variant` is `"real"` (default, omittable) or `"debug"`. It is the
invocation flavour the daemon relays from
`underwrap-<instance> <wrapper>` vs `underwrap-<instance> debug <wrapper>`. The
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
  `writable` flag (deny `not-writable` if false), rejects decoded complete
  blobs above 2,097,152 bytes with `resource-too-large` before parsing or
  persistence, then re-canonicalizes the blob (deny `noncanonical-blob` if it doesn't match
  the submitted bytes), rejects unknown `schema_version` values, then
  compares the submitted `env` section with the resource blob loaded during
  finalize. A difference rejects with
  `InvalidBlob { reason: "env-section-immutable" }`.
- On an accepted write, the resource file is atomically replaced and
  `last_used_at` is bumped. Policy denials and invalid blobs do not count as a
  successful finalize and do not bump it.

The compare and replacement are not a transaction or compare-and-swap.
Concurrent whole-resource writers are last-writer-wins; in particular, a stale
runtime writeback can overwrite an admin env edit that lands between finalize's
load and save. Serialize edits to one resource operationally. See
[Concurrency semantics](#concurrency-semantics).

Tagged response variants:

```json
{ "outcome": "ok" }
{ "outcome": "deny", "reason": "not-writable" }
{ "outcome": "invalid-blob", "reason": "env-section-immutable" }
```

Structured `InvalidBlob` reasons: `decode-failed`, `noncanonical-blob`,
`schema-unknown`, `resource-too-large`, `env-section-immutable`. They are split apart from
`Deny` because a `Deny` may flip to `Allow` after an operator edits
the policy, whereas `InvalidBlob` is a client bug that won't change
on retry.

Store selection is implicit in the authenticated principal. There is
no store name in any request body. Operators create runtime client
instances directly in the admin UI by pasting a cert PEM the helper
produced; the helper does not talk to the backend.

A runtime handler's generic internal failure returns HTTP 500 with code `internal` and exact message `the backend could not complete the request`.
The corresponding `runtime internal failure` ERROR event records only `response_code`, static `context`, and concrete Rust `error_type`, never the source value's display text.
Runtime storage requests receive `503 store_busy` while migration owns the store lifecycle and `500 migration_recovery_required` while durable migration state awaits trusted-host recovery.

## Admin API

The admin API lives on the same HTTPS listener as the runtime API and
shares the same server certificate. Operator requests authenticate with
session cookies; state-changing requests must also satisfy the CSRF and
`allowed_origins` checks.

A generic admin internal failure returns HTTP 500 with code `internal` and exact message `the backend could not complete the request`.
The audited store and runtime-client registry failures retain their existing codes but use these stable public messages:

| Code | Public message |
| --- | --- |
| `list_store_failed` | `could not inspect store contents` |
| `create_data_dir_failed` | `could not create store data directory` |
| `open_store_failed` | `could not open store` |
| `canary_write_failed` | `could not initialize encrypted store` |
| `write_store_registry_failed` | `could not persist store registry` |
| `write_client_instances_failed` | `could not persist client instance registry` |

Other source-less code-specific 500 responses retain their static public guidance.
Every source-bearing admin 500 constructed through these internal-error helpers emits an `admin internal failure` ERROR event with `response_code`, static `context`, and concrete Rust `error_type`.
Only the audited filesystem, storage-open/list/write, canary, and registry-persistence failures add the source's alternate-Display `error` field, which can contain trusted-host paths and non-secret identifiers.
Generic failures never format arbitrary source values because malformed stored input can appear in an error's display text.
The response contains none of these diagnostic fields or source details, and the fields do not provide a per-request correlation identifier.

The OpenAPI 3.1 document at
`crates/underwrap-backend/openapi.json` is the generated client contract for
the documented admin routes, DTO schemas, and responses declared on annotated
handlers. It is derived from `#[utoipa::path(...)]` annotations plus
`#[derive(utoipa::ToSchema)]` on DTOs. It is not an exhaustive inventory of
errors produced around handlers by shared middleware, extractors, or
infrastructure, and it does not model or enforce the `allowed_origins` gate;
the live router and its tests remain authoritative for those behaviors. The
mTLS runtime API (`/v1/wrappers`, `/v1/wrapper-invocations*`) is deliberately
*not* in the spec — it is an internal daemon-to-backend contract, not a
front-end consumable.

Regenerate the spec by running `cargo test --workspace`. The semantic tests in
`openapi_export.rs` verify the generated security and lifecycle meaning, and its
byte-drift test rewrites `openapi.json` in place and fails until the file is
committed. Live handler tests remain the authority for statuses and canonical
codes actually emitted by handlers.

The SPA's API client (`web/src/api/client.ts`, built on
`openapi-fetch`) consumes the same spec: `npm run generate:api`
re-runs `openapi-typescript` against `openapi.json` to produce the
ignored local file `web/src/api/schema.ts`. The `predev`, `prebuild`,
and `pretest` npm hooks run this automatically, so a stale spec
breaks the SPA build at the earliest possible point. Within the generated
client contract's scope, the OpenAPI schema supplies the SPA's typed route, DTO,
and annotated-response surface.

Two SecuritySchemes are referenced from individual operations:
`session_cookie` (the opaque `__Host-underwrap-session` cookie issued by
tower-sessions) and `csrf_header` (the synchronizer `X-CSRF-Token` header
issued by `axum_csrf`). The document root has no non-empty security
requirement. Effective operation security follows OpenAPI inheritance: an
absent operation-level field inherits the root, while an explicit empty array
is public.
The exact effectively public set is login, whoami, and read-only bootstrap status.
Every other `GET` carries one session-only Security Requirement Object.
Each of the 34 protected state-changing operations carries one Security Requirement Object containing both schemes, which means session **and** CSRF in OpenAPI rather than two alternative (OR) requirements.

The exact 32 store-backed operations that can encounter locked or busy state advertise `503 Service Unavailable` with `store_locked` and/or `store_busy` in their annotated contract and a `ServiceErrorResponse`.
The exact 32 store-backed operations that can encounter durable migration state advertise `500 migration_recovery_required`.
The admin contract advertises no `423 Locked` response.

The full route table follows, grouped by resource and using the real HTTP
verbs. It is a human-readable index of the generated client route surface;
handler and live-router tests cover runtime behavior outside that surface. Path
parameters: `{id}`/`{sid}` is a store, `{rid}` a resource, `{cid}` a libsecret
collection, `{iid}` a libsecret item.

### Authentication and session

- `GET  /v1/admin/auth/bootstrap/status` — *public and read-only*; reports
  whether the deployment operator still needs to run the offline
  `underwrap-backend admin bootstrap-user` command. No network route creates an
  operator.
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

Each list item carries its stable id, metadata, backend kind, lifecycle state, usage projection, and server-derived action fields.

| Lifecycle state | Meaning | Allowed store actions |
| --- | --- | --- |
| `ready` | Plain backend available | Edit, migrate to encrypted, and usage-dependent delete |
| `unlocked` | Encrypted backend and key available | Edit, migrate to plain, rotate, lock, and usage-dependent delete |
| `locked` | Encrypted backend without an installed key | Edit, unlock, and conservative force-delete |
| `busy` | Migration owns lifecycle publication | No mutation action |
| `recovery-required` | Durable migration state needs trusted-host recovery | No mutation action |

Locked, busy, and recovery-required list items do not inspect store contents and do not report known emptiness.

- `GET    /v1/admin/stores` — list stores with lifecycle and action projections.
- `POST   /v1/admin/store-keys` — generate a fresh key without mutating storage.
  The response is `200 { key_hex }` with `Cache-Control: no-store, no-cache, must-revalidate`, `Pragma: no-cache`, and `Expires: 0`.
- `POST   /v1/admin/stores` — create a store.
  Encrypted creation requires `key_hex` as exactly 64 lowercase hexadecimal characters without trimming or normalization, writes the encrypted canary, leaves the store `unlocked`, and returns only the store projection.
  Plain creation rejects a supplied key and leaves the store `ready`.
  Omitted `key_hex` remains valid input where the direction permits it, while explicit JSON `key_hex: null` is `400 invalid_request` before mutation for every backend kind.
  Other conditional key failures are `400 key_required`, `unexpected_key`, or `invalid_key_hex`.
- `PUT    /v1/admin/stores/{id}` — update operator-facing metadata without changing the stable id or backend kind.
- `DELETE /v1/admin/stores/{id}` — delete the store record when mapping and content rules allow it.
  Locked stores require `?force=true` because their contents cannot be inspected.
- `POST   /v1/admin/stores/{id}/migrate` — convert the backend at the unchanged canonical path while preserving the stable id, metadata, runtime client mappings, control documents, resource ids, and logical bytes.
  Plain-to-encrypted migration requires the same exact key contract as encrypted creation and leaves the store `unlocked`.
  Encrypted-to-plain migration requires an unlocked source, accepts no key, and leaves the store `ready`.
  The endpoint returns `400 invalid_request`, `key_required`, `unexpected_key`, or `invalid_key_hex`; `404 not_found`; `409 same_backend`; `503 store_locked` or `store_busy`; or one fixed `500` failure from the table below.
  `500 migration_recovery_required` takes precedence over `409 same_backend`, including when recovery publication has already persisted the requested target backend kind.
- `POST   /v1/admin/stores/{id}/unlock` — body `{key_hex}` decrypts the canary to verify the key and installs the `unlocked` backend.
  Surrounding key whitespace remains tolerated only for unlock.
  The exact response set is `{200, 400, 403, 404, 409, 500, 503}`: 400 carries `invalid_key_hex` or `store_not_encrypted`; a syntactically valid wrong key is `403 key_mismatch`; 404 carries `not_found`; 409 carries `already_unlocked`; 500 carries `open_store_failed`, `rotation_recovery_required`, or `migration_recovery_required`; and 503 carries `store_busy`.
- `POST   /v1/admin/stores/{id}/lock` — drop the in-memory key so subsequent storage requests fail `503 store_locked`.
  The operation is idempotent for an already-locked encrypted store and also reports migration `busy` or `recovery-required` states distinctly.
- `POST   /v1/admin/stores/{id}/rotate-key` — stage and verify a complete encrypted copy under a fresh internally generated master key, activate it, and return the new `key_hex` exactly once.
  Rotation acquires the shared per-entry lifecycle mutex but retains its existing non-draining behavior for ordinary reads and writes.
  Quiesce wrapper traffic and admin mutations before rotation because concurrent reads can fail and concurrent writes can be lost.
  The displaced ciphertext remains in a unique recovery workspace and is never automatically deleted.

Migration failures use fixed value-free public responses.

| Failure disposition | HTTP status and code |
| --- | --- |
| Workspace or manifest reservation fails before displacement | `500 migrate_stage_failed` |
| Source listing fails before displacement | `500 migrate_list_failed` |
| Source or target canary validation fails before displacement | `500 migrate_canary_failed` |
| A source value cannot be read before displacement | `500 migrate_read_failed` |
| A target value cannot be written before displacement | `500 migrate_write_failed` |
| The staged or promoted id set or logical bytes do not verify | `500 migrate_verify_failed` |
| Source displacement, target promotion, or checked rollback rename fails | `500 migrate_swap_failed` when a verified source is restored, otherwise `500 migration_recovery_required` |
| The promoted target cannot be reopened | `500 migrate_reopen_failed` when a verified source is restored, otherwise `500 migration_recovery_required` |
| Store-record persistence fails | `500 write_store_registry_failed` when the verified source and source record are restored, otherwise `500 migration_recovery_required` |
| Secret-bearing workspace cleanup partially or wholly fails | `500 migration_recovery_required` |
| Existing leases do not drain within 30 seconds | `503 store_busy` with the unchanged source restored |
| Startup finds an exact migration workspace | `500 migration_recovery_required` for storage access until trusted-host recovery |

A phase-specific error is returned only when rollback and cleanup leave one verified source authoritative and no ambiguous split state.

#### Migration admission and publication

Migration acquires the exact entry-owned lifecycle mutex, re-resolves the entry, and publishes `busy` while briefly holding the store-registry write lock.
The busy publication blocks later runtime and admin storage leases, while leases admitted earlier retain an entry-owned count until their complete handler operation ends.
Migration waits up to 30 seconds for that count to reach zero before reading the source or changing the filesystem.
No registry lock is held across lifecycle-mutex waiting, lease drainage, storage I/O, filesystem mutation, or store-record persistence.
After drainage, the synchronous copy, verification, filesystem, persistence, cleanup, and publication window runs inside a runtime-aware `block_in_place` boundary on the production multi-thread runtime and contains no asynchronous cancellation point.
An invocation opened before migration can still finalize afterward under the existing stateless last-writer-wins contract, so the operator must quiesce wrapper traffic and admin mutations before starting migration.

For canonical store directory `<parent>/<store-id>`, migration exclusively creates:

```text
<parent>/.<store-id>.underwrap-migration-<16-lowercase-hex-id>/
```

The workspace and newly created `stage/` target are regular non-symlink directories with exact mode `0700`; setuid, setgid, sticky, or any other mode bit causes validation to fail.
The renamed `backup/` inherits the canonical source mode while remaining beneath the private workspace, and mode drift prevents cleanup and requires trusted-host recovery.
Its versioned `manifest.json` contains only the stable store id and source and target backend kinds.
Every canonical, stage, backup, rollback, and cleanup path receives checked occupancy, directory, and symlink validation before rename or removal.

The backend retains the source id inventory but never a whole-store plaintext value map.
It reads, copies, and drops one logical source value at a time, then verifies and drops one source/target logical-value pair at a time while checking complete id sets.
While the original source is live, migration also computes a domain-separated SHA-256 fingerprint over the source backend kind, explicit canary treatment, complete sorted storage-id sequence, unambiguous lengths, ids, and every logical value byte.
Only that fixed-size digest and the id inventory remain in memory as bounded rollback-integrity evidence; this does not claim collision impossibility or protection from malicious backend-host root.
Plain-to-encrypted migration rejects a source canary and writes and verifies the canonical encrypted canary in the target.
Encrypted-to-plain migration verifies and fingerprints the source canary and omits it from the plain target.
After target verification, publication moves the source to `backup/` and reopens that displaced generation through the same `StorageBackend` kind without exporting encrypted key bytes.
The backend requires the reopened source to match the captured fingerprint before trusting it, immediately before and after moving a failed promoted target aside, and after restoring the source at the canonical path.
A source fingerprint mismatch, unreadable value, or id mismatch requires recovery and preserves the workspace rather than being reported as target-verification failure.
Direct source-to-target comparison still checks one pair at a time before and after promotion.
It promotes `stage/` to the unchanged canonical path, reopens and re-verifies the target, atomically persists the same store record with only its backend kind changed, removes the displaced source and workspace, publishes the target live entry, and returns success.

| Direction | Plaintext recovery residue after failure or interruption |
| --- | --- |
| Plain to encrypted | The displaced plaintext source can remain under `backup/` after displacement, rollback failure, registry failure, cleanup failure, or interruption |
| Encrypted to plain | The plaintext target can remain under `stage/` or at the canonical path after staging, promotion, cleanup failure, or interruption |

Both residue classes remain under mode-`0700` workspace or store directories, remain outside runtime and admin resource APIs while recovery is required, and require trusted-host handling.
Successful plain-to-encrypted migration leaves no intentional plaintext migration pathname, but path removal is not secure media erasure and does not prove prior plaintext blocks are unrecoverable.

#### Backend migration recovery

Use this procedure only for a store in `recovery-required` after an interrupted or failed backend migration or after startup detects an exact migration workspace.
Recovery is a trusted backend-host operation and is not available through the browser or admin API.

1. Stop `underwrap-backend` and every other process that can access the selected store, store registry, canonical data path, or sibling migration workspaces, then verify that the HTTPS listener and writers are stopped.
2. Inventory the store record, canonical path, and every sibling matching `.<store-id>.underwrap-migration-<16-lowercase-hex-id>` without deleting, overwriting, following symlinks, or normalizing a near-match into the exact namespace.
3. Preserve every canonical, `stage/`, and `backup/` generation and validate the non-secret manifest when possible to identify its recorded store id and source and target backend kinds, while treating a missing or malformed manifest as recovery state rather than authority.
4. Reject symlinks and non-directory generation roots, retain exact mode `0700`, and use non-overwriting quarantine names for every move so no unselected state is displaced or destroyed.
5. Use trusted offline tooling built against the matching release's `StorageBackend` implementations to open each candidate with a backend kind established from the valid manifest, persisted record, and source or target role rather than its pathname alone, supply the matching externally held key for an encrypted candidate, verify the encrypted canary or plain canary absence, list the complete id set, and read every logical value.
6. Select one verified generation whose control documents, resources, and logical bytes match the intended source or target state, and stop without guessing if no candidate and matching encrypted key can be verified.
7. Quarantine any unselected canonical generation under a fresh occupied-checked mode-`0700` sibling name, then move the selected generation to the canonical path with checked non-overwriting same-filesystem renames.
8. Rename every remaining exact migration workspace to a fresh nonmatching mode-`0700` quarantine sibling with checked non-overwriting moves, preserve all of its contents, and verify that no exact migration workspace name remains to trigger startup detection.
9. Atomically replace the existing store record so its file stem, name, description, `created_at`, and canonical `dir` remain unchanged and only `backend` matches the selected generation.
10. Restart the backend, unlock an encrypted selected generation with its matching key, and verify the stable store projection, representative control and resource reads, runtime client mappings, and a representative authorized runtime read.
11. If any verification fails, stop the backend again and preserve all canonical, quarantined, and workspace state before choosing another previously verified candidate.
12. Delete only explicitly selected obsolete migration artifacts after the chosen generation and mappings have been verified, and leave unrelated retained rotation workspaces untouched.

Underwrap does not ship an automatic migration-recovery selector or cleanup command.
This procedure requires trusted tooling capable of opening and exhaustively reading a candidate through the matching backend implementation.
It cannot restore media loss, arbitrary corruption, a missing encrypted-generation key, or effects that were never flushed before power loss.
Recursive cleanup may already have partially removed an obsolete generation before reporting failure, so recovery does not promise that every earlier file remains present.
The procedure provides no transaction, compare-and-swap, parent-directory fsync, power-loss durability, or secure-erasure guarantee.

#### Retained rotation generations

For canonical store directory `<parent>/<store-id>`, rotation exclusively
creates a mode-`0700` workspace named:

```text
<parent>/.<store-id>.underwrap-rotation-<16-lowercase-hex-id>/
```

Before the swap it contains `stage/`. After success it contains `backup/`, the
complete pre-rotation generation, while the fresh generation occupies the
canonical path. Later rotations create new workspaces and never discover,
overwrite, modify, or delete earlier ones. A failed rotation whose rollback
succeeds restores the previous canonical generation and leaves the complete
unpublished new generation at `stage/`; it is never selected automatically and
requires explicit trusted-host cleanup. A failed rollback can leave both
`backup/` and `stage/`, or `backup/` plus a canonical promoted generation.
Startup does not enumerate or reconcile these paths. Store deletion likewise
does not remove retained sibling workspaces.

The backend cannot know whether an HTTP response reached the operator or whether
the replacement key entered external custody. Retention is therefore unbounded:
it consumes disk and every retained snapshot remains decryptable with its old
key until a trusted host operator deliberately removes it.

To recover one generation, use backend-host authority while all store access is
quiesced and the backend is stopped:

1. Enumerate only workspaces matching the exact store id and naming rule. Select
   a workspace whose `backup/` corresponds to a key already held externally.
2. Choose a fresh sibling name for the current canonical directory. Treat a
   dangling symlink as occupied: require both `test ! -e PATH` and
   `test ! -L PATH` for every destination. Verify the selected `backup/` is a
   directory. The canonical path may already be absent after an interrupted
   rename; do not create it or require a live generation to exist.
3. Refuse a canonical symlink, including a dangling one. If the canonical path
   exists, require it to be a directory and preserve it at the fresh sibling
   with a checked, non-overwriting, no-target-directory move such as
   `mv --no-clobber --no-target-directory SOURCE DESTINATION`; then assert the
   source is absent and destination is present. If canonical was already absent,
   leave it absent. Promote the selected `backup/` with the same checked
   move, after rechecking that canonical is neither present nor a dangling
   symlink; assert `backup/` is absent and canonical is a directory afterward.
   Never delete or overwrite a path to make room.
4. Start the backend. It leaves the encrypted store locked. Unlock with the
   selected generation's key and verify the canary plus representative resource
   and control-document reads through normal APIs.
5. If verification fails, stop again and preserve every path before restoring or
   trying another generation. There is no automatic selection or promotion.

Cleanup is a separate trusted decision. First verify a later active key through
lock or restart, unlock, canary, and representative resource/control reads.
Stop the backend, re-enumerate exact paths, and remove only the explicitly
selected obsolete workspace or preserved generation. Confirm that the canonical
path and all other retained workspaces are unchanged before restarting. Do not
infer safe cleanup from HTTP 200 or the browser checkbox.

These steps provide process-visible recovery across returned I/O errors,
interruption states, restart, and response loss. They do not provide CAS,
coordinate ordinary writes, guarantee response delivery, or promise fsync/power-
loss durability.

### Resources

First-class `ResourceRecord`s that own name provenance.

- `GET    /v1/admin/stores/{id}/resources` — list.
- `POST   /v1/admin/stores/{id}/resources` — create.
- `PUT    /v1/admin/stores/{id}/resources/{rid}` — update.
- `DELETE /v1/admin/stores/{id}/resources/{rid}` — delete; refuses with one
  structured `blocked_by` response while any stored env entry, file entry,
  libsecret alias, collection (including an empty collection), item,
  wrapper/resource pair, or permission row remains. `content_entry_count` is
  the complete saturated total and `sample_content` is a deterministic capped
  tagged sample (`env`, `file`, `libsecret_alias`, `libsecret_collection`, or
  `libsecret_item`). Samples contain identifiers only; identifiers can still be
  sensitive operator input and are never copied into audit events or logs.

### Wrapper/Resource pairs

- `GET    /v1/admin/stores/{id}/pairs` — list.
- `POST   /v1/admin/stores/{id}/pairs` — create against an exact resource
  record in the selected store; a missing resource returns the canonical
  value-free `404 not_found` / `unknown resource` envelope, and an existing
  `(wrapper, resource)` remains a conflict.
- `POST   /v1/admin/stores/{id}/pairs/with-new-resource` — create a resource
  and pair in one API call. Their two control documents are saved separately;
  failure of the second save triggers best-effort rollback of the first.
- `DELETE /v1/admin/stores/{id}/pairs/{wrapper}/{rid}` — delete; refuses
  while any wrapper permission references the pair.

### Secrets (per-slot entries inside a resource blob)

Direct access to a store's per-secret surface, addressed by resource id
+ slot + key. Admin writes are unconditional — there is no global write
toggle (see [No global write lock](#no-global-write-lock)).

- `GET    /v1/admin/stores/{id}/secrets` — list entries (keys, never
  values). With `?resource_id=...`, the exact catalog resource must exist;
  without that filter the store-wide list remains a diagnostic view and can
  expose pre-existing orphan blob keys.
- `GET    /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}` — read one
  entry.
- `PUT    /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}` — create or
  overwrite one entry.
- `DELETE /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}` — delete.
- `POST   /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}/rename` — rename one
  entry in one in-memory mutation followed by an atomic resource-file
  replacement.

Every explicit resource-addressed operation above requires an exact
`ResourceRecord` in the selected store before reading, listing, writing, or
deleting its active resource blob. A missing catalog resource returns the
value-free `404 not_found` / `unknown resource` envelope. An existing catalog
resource with no active blob still starts as an empty `ResourceBlob`; subordinate
missing-key and idempotent-delete semantics are unchanged.

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

All eight operations pass through shared read/mutation seams that require the
exact parent `ResourceRecord` before active blob access. Missing parents receive
the same value-free resource 404 as the flat routes. Existing empty resources,
subordinate collection/item errors, and idempotent collection/item/alias deletes
retain their documented behavior.

### Wrapper permissions (the policy surface)

- `GET    /v1/admin/stores/{id}/wrapper-permissions` — list every
  permission row in the store.
- `PUT    /v1/admin/stores/{id}/wrapper-permissions/{client_instance}/{wrapper}/{rid}`
  — create or update one permission by
  `(client_instance, wrapper, resource)` tuple. After non-empty tuple
  validation and selected-store resolution, the backend requires, in order, a
  currently published runtime client instance assigned to that store, the exact
  resource record, and the exact wrapper/resource pair. Disabled instances are
  valid references. A missing reference returns its value-free canonical 404
  before the policy document is loaded or changed.
- `DELETE /v1/admin/stores/{id}/wrapper-permissions/{client_instance}/{wrapper}/{rid}`
  — remove one permission by tuple.

### Denormalized views (SPA convenience reads)

- `GET /v1/admin/stores/{id}/secrets-view` — secrets joined with their
  resource/pair provenance.
- `GET /v1/admin/stores/{id}/wrapper-permissions-view` — permissions
  joined with resolved wrapper / resource / client-instance names.

Each low-level storage write atomically replaces the affected document or
resource file. Multi-document operations and higher-level load-modify-save
cycles are not transactions; see [Concurrency semantics](#concurrency-semantics).

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
    reason: "maintenance window closed"
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
There is no edit endpoint; `delete` on either entity refuses when dependents
exist. Resource deletion classifies the complete typed `ResourceBlob`, not the
flat env/files admin projection, and aggregates content, pair, and permission
counts in one refusal. Its capped content sample carries only raw map
identifiers; values, labels, alias targets, attributes, content types,
timestamps, and secret bytes are not copied. Identifiers are not inherently
secret-free, so blocker samples are returned only to the authenticated operator
and are never copied into audit events or logs.

After every blocker load and decode succeeds and all counts are zero, deletion
requests removal of the active `resources/<resource_id>` key before removing
and saving the catalog record. A missing key is the valid never-materialized
empty case. The backend returns `ok: true` and emits the success audit event only
after both operations report success. A blob-delete error prevents the catalog
write. If the later catalog save reports an error, no blob reconstruction is
attempted; storage errors do not prove which attempted effects became visible,
so operators must re-read the active blob key and catalog state before a
serialized reconciliation or retry.

`last_used_at` (unix seconds, `None` until first use) is bumped
server-side after a successful open and after a successful finalize
(writeback or usage signal). Policy denials and invalid writeback blobs do not
bump it. The bump is idempotent at one-second resolution and a failure to update
it does not fail the otherwise successful request. Mount paths are a
daemon-local concern derived from wrapper config; they never travel on the wire
and the pair record does not store them.

## Resource blob storage

Every resource is stored as **one** opaque key in the
`StorageBackend` at `resources/<resource_id>`. The value is the
JCS-canonical (RFC 8785) JSON bytes of a `ResourceBlob`, capped inclusively at
2,097,152 bytes as a complete resource, carrying the
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

`Collection.created`, `Collection.modified`, `Item.created`, and `Item.modified` are unsigned Unix-second values accepted from zero through `9,007,199,254,740,991` (`2^53 - 1`) inclusive.
Encoding and decoding reject any of those four fields above that maximum before the resource can enter canonical storage or transport.

`BTreeMap` keeps key order deterministic; combined with `serde_jcs`, the
canonical bytes are byte-identical for any two logically equal blobs. At the
end of a wrapper run, `underwrap-ns` canonicalizes the composed resource and
compares it byte-for-byte with the Checkout bytes. Equal bytes produce a
`UsageSignal`; changed bytes produce a `Checkin` when writable.

Keys in the `files` section are full relative paths inside the
target's home view (e.g. `.config/gh/hosts.yml`). The backend treats
them as opaque strings — it only enforces the usual no-traversal /
no-leading-slash hygiene — but the frontend wrapper's `files.paths`
determines which keys are projectable. The first path component(s)
of every key must match one of the wrapper's configured mount paths;
ns aborts the invocation if any blob key has no matching mount.

`PlainBackend` stores the blob as one file per resource;
`EncryptedBackend` AEAD-encrypts the blob as one ciphertext with one
nonce per resource. Successful resource deletion makes the active empty blob
key absent in either backend; it is not a secure-media-erasure claim and does
not delete copies in retained encrypted rotation generations.
The operator-facing per-secret admin endpoints
(`/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}`) keep the
`<resource>/<slot>/<key>` address space but route through the blob
via a load-modify-save shim. **No runtime endpoint accepts a storage
key on the wire.**

### Concurrency semantics

`PlainBackend` and `EncryptedBackend` atomically replace one stored value, so a reader does not observe a partially written file or ciphertext.
The backend provides no transaction, version, compare-and-swap primitive, or per-resource mutation lock above that storage operation.

Consequently, concurrent load-modify-save operations against the same resource
or control document are last-writer-wins. This includes runtime finalize racing
an admin content edit or resource deletion, two admin edits, registration racing
resource/pair/policy changes, and an admin env edit racing a stale runtime
writeback. The finalize env comparison protects against a submitted env change
relative to the snapshot it loads; it does not protect an edit that lands
between that load and the whole-blob save. Operators and API clients must
serialize all relevant runtime writeback and admin content, resource, pair, and
permission mutations for a lifecycle operation; deletion ordering is not a
transaction or compare-and-swap guarantee.

For serialized requests, explicit flat/libsecret content operations reject a
missing catalog resource, ordinary pair creation rejects a missing resource,
and permission upsert rejects a client/store, resource, or exact-pair reference
that is absent when checked. These are check-then-use rules, not transactions:
the catalog, published client registry, pairs document, policy document, and
resource blob are read or written separately. A concurrent deletion,
reassignment, registration, or mutation can invalidate a successful check before
the later blob, pair, or policy write and can therefore recreate an orphan blob
or dangling row. There is no tombstone or global post-delete exclusion.

Operators must serialize those writers through deletion and keep them stopped
until the resulting state has been observed. The backend does not scan, hide,
delete, migrate, garbage-collect, or synthesize catalog entries for pre-existing
or concurrently produced orphan blobs, dangling pairs, or stale permissions;
unfiltered aggregate/list endpoints and permission list/delete remain diagnostic
and cleanup surfaces rather than implicit repair.

Store migration and key rotation have the stronger operational requirements described under [Stores](#stores).
Migration drains normal storage leases admitted before `busy` but cannot retract a later finalize from an invocation opened earlier, while rotation retains its existing non-draining behavior.
Quiesce wrapper traffic and admin mutations for the selected store before either operation.

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

