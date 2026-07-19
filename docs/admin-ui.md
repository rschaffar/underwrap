# Admin UI

The admin UI is the operator interface for backend users, stores, runtime client
instances, resources, wrapper/resource pairs, secret entries, and permission
rows. It is a Vite/React SPA embedded in `underwrap-backend` and served from the
same HTTPS listener as `/v1/admin/*`.

## Open the UI

Browse to the backend's configured HTTPS origin, for example:

```text
https://backend.example.internal:18443/
```

The browser must trust the backend's server certificate. No runtime client
certificate is required for browser access.

## First-use bootstrap

An empty backend redirects to `/bootstrap`, which shows deployment instructions
but accepts no credentials. Stop the backend service and run, as its service
identity:

```text
underwrap-backend --config /etc/underwrap/backend.yaml admin bootstrap-user --username <name>
```

Supply the password through standard input from a trusted prompt or password
manager; never place it in argv, environment variables, configuration, or shell
history. The command succeeds only while the operator collection is empty and
the bootstrap sentinel is absent. Restart the service and sign in normally.

All authenticated operators have the same authority. There are no roles or
per-store operator grants. A last-user guard prevents deleting the final
operator account.

## Stores

The **Stores** page creates and manages storage backends.

### Plain store

A plain store persists resource blobs as plaintext files under its configured
data directory. File permissions and backend-host security protect the data.

### Encrypted store

Creating an encrypted store returns a new 32-byte master key as hexadecimal
**exactly once**.

1. Copy the key directly into an external password manager or vault.
2. Confirm that it is recoverable before leaving the response.
3. Do not store it beside the backend data directory.

The backend never writes this key to disk. An encrypted store starts locked
after every backend restart. Use **Unlock** and provide the key before runtime
or admin secret operations can access it.

**Lock** drops the in-memory key. **Rotate key** re-encrypts the store and
returns the replacement key exactly once; update external custody immediately.
Quiesce wrapper traffic and other admin access first: rotation is serialized
only against another rotation, so concurrent reads may fail and concurrent
writes may be lost. There is no recovery mechanism for a lost key.

Store creation is available only when `admin.stores_data_root` is configured.

## Runtime client instances

A runtime client instance represents one frontend daemon identity and maps to
one store.

On the frontend host, generate a self-signed client credential:

```bash
sudo underwrap-keygen \
  --instance-config /etc/underwrap/daemon/<instance>.yaml
```

In **Client instances**, create a record with:

- an operator-facing display name;
- the target store;
- `Active` or `Disabled` status;
- the complete certificate PEM printed by `underwrap-keygen`;
- the wrapper-asset auto-create choice.

The private key never leaves the frontend host. The backend stores the
certificate and authenticates later runtime connections by its SHA-256
fingerprint.

### Wrapper-asset auto-create

When enabled, `POST /v1/wrappers` may create missing resources and
wrapper/resource pairs for this client instance. Registration also seeds a
`Locked` permission row for each resolved pair.

When disabled, registration reports missing resources/pairs without creating
them. Create them manually through **Secrets** and **Wrapper permissions**.

Auto-create is convenient for initial enrollment; disabling it gives operators
an explicit asset-creation workflow.

Changing a client instance's target store removes that client's permission rows
from the preceding store. Replacing its certificate rotates the runtime
credential. Disabling or deleting the instance stops its certificate from
authenticating.

## Resources and secret entries

A resource is the unit one wrapper invocation projects. It contains three
slots:

- `env` — environment variable name/value entries;
- `files` — relative path/byte entries;
- `libsecret` — collections, items, aliases, attributes, and secret payloads.

The **Secrets** page can:

- create and rename resources;
- add, reveal, edit, rename, and delete env/files entries;
- manage the structured libsecret tree;
- delete a resource after all dependent pairs and permissions are removed.

Admin writes are unconditional for an authenticated operator. The permission
writable flag controls wrapper-side files/libsecret writeback; it does not
restrict admin edits.

Resource names are operator-facing labels. Backend records use stable generated
resource IDs so a rename does not invalidate permissions.

## Wrapper/resource pairs

A pair declares that one wrapper name can address one resource. The
**Wrapper permissions** page shows pairs grouped by resource and can create or
delete them.

Pair deletion is blocked while a permission row references the pair. Resource
deletion is blocked while secrets, pairs, or permission rows depend on it. The
UI reports those blockers so they can be removed in order.

## Permission rows

Click the cell for a runtime client instance and wrapper/resource pair to create
or edit its permission row.

| Permission mode | Real wrapper | Debug variant |
| --- | --- | --- |
| `Locked` | denied | denied |
| `Unlocked` | allowed | denied |
| active `UnlockedUntil` | allowed | denied |
| active `UnsafeDebugUntil` | allowed | allowed |

Expired time-bounded modes deny both variants. The backend checks expiry on each
request.

The **writable** flag controls persistence of files/libsecret changes. Runtime
finalize rejects an env section that differs from the resource snapshot it
loads. Resource updates are whole-blob and last-writer-wins, however, so avoid
editing a resource while one of its writable wrapper invocations is finalizing.

### Unsafe debug mode

`UnsafeDebugUntil` permits `underwrap-debug-tool` for the selected wrapper. It
can show an env summary, start an interactive shell, or execute an arbitrary
command inside the wrapper namespace with the projected resource available.
When writable is enabled, debug changes can be persisted.

The UI requires explicit confirmation before setting this mode. Use the shortest
practical deadline and return the permission to `Locked`, `Unlocked`, or
`UnlockedUntil` immediately afterward.

## Users and profile

- **Users** creates, renames, deletes, and resets passwords for operator
  accounts.
- **Profile** changes the signed-in operator's password after verifying the
  current password.

Password change, password reset, and user deletion invalidate that user's
existing sessions on their next request.

## Authentication and browser security

The backend uses:

- Argon2id password hashing;
- opaque `__Host-underwrap-session` cookies with `Secure`, `HttpOnly`,
  `SameSite=Strict`, and `Path=/`;
- server-side sessions in `tower-sessions::MemoryStore`;
- synchronizer-token CSRF through a bound cookie and `X-CSRF-Token` header;
- optional `admin.allowed_origins` checks on state-changing requests;
- per-username login rate limiting and a dummy Argon2 verification path for
  unknown usernames.

Sessions disappear when the backend restarts, so operators must sign in again.

Routes are:

| Page | Path |
| --- | --- |
| Secrets | `/secrets` |
| Wrapper permissions | `/wrappers` |
| Stores | `/stores` |
| Client instances | `/client-instances` |
| Users | `/users` |
| Profile | `/profile` |
| Login | `/login` |
| Bootstrap | `/bootstrap` |

`/` redirects to `/secrets` after authentication.

## Local frontend development

From the repository:

```bash
nix develop
cd web
npm ci
npm run build
cd ..
cargo run -p underwrap-backend -- --config /path/to/config.yaml
```

In another terminal, point Vite at that backend:

```bash
cd web
UNDERWRAP_BACKEND_DEV_URL=https://127.0.0.1:18443 npm run dev
```

The Vite development server proxies `/v1/*` to
`UNDERWRAP_BACKEND_DEV_URL` (default `http://127.0.0.1:9080`). The normal
browser authentication and CSRF flow remains in use.

Validation commands:

```bash
cd web
npm test
npm run build
npm run test:e2e
```

The Playwright command runs inside `nix develop`. See [Testing](testing.md).

## API contract

`crates/underwrap-backend/openapi.json` is the generated OpenAPI 3.1 contract
for the admin API. The SPA generates its TypeScript schema from that document.
See [Backend](backend.md#admin-api) for the route index and data model.
