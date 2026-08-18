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
returns the replacement key exactly once. Every displaced ciphertext generation
remains in a unique backend-host recovery workspace. It consumes disk and stays
decryptable with its old key until a trusted host operator removes that selected
artifact after independently verifying a later key through lock/restart,
unlock, and representative reads. The reveal checkbox prevents accidental local
dismissal only; it does not tell the backend that delivery or external custody
succeeded.

Quiesce wrapper traffic and other admin access first: rotation is serialized
only against another rotation, so concurrent reads may fail and concurrent
writes may be lost. If a response is lost, the old key still opens the retained
pre-rotation snapshot through the trusted-host procedure in
[Backend](backend.md#retained-rotation-generations). Recovery and cleanup are
not browser or API operations.

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
from the preceding store. Disabling or deleting the instance stops its
certificate from authenticating.

### Replace a runtime client certificate

Certificate replacement is one ordered operator cutover for the existing
runtime client instance:

1. On the frontend, successfully generate and atomically install the replacement
   combined identity:

   ```bash
   sudo underwrap-keygen \
     --instance-config /etc/underwrap/daemon/<instance>.yaml
   ```

2. In **Client instances**, edit that same instance, paste the public certificate
   printed by the successful command, and select **Save**.
3. On the frontend, load the replacement identity explicitly:

   ```bash
   sudo systemctl restart underwrapd-<instance>.service
   ```

4. Verify a representative wrapper for that instance.

The running daemon continues using its cached old identity until step 3. Saving
the replacement certificate makes the backend reject that old identity, so an
interruption is expected between **Save** and the restart: wrapper requests fail
with enrollment guidance. Only the public certificate crosses to the backend;
the private key remains in the frontend's combined identity file. Identity-file
installation guarantees and non-transactional boundaries are defined in
[Security](security.md#runtime-client-identity-installation). This operation does
not apply one timing promise to display-name, store, status, or wrapper-asset
auto-create edits.

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
- delete a resource after all stored resource content, dependent pairs, and
  permission rows are removed.

Secret inputs in this browser UI are text-only. The browser encodes JavaScript
text losslessly as UTF-8 bytes before base64 transport; it does not apply Unicode
normalization or line-ending conversion. Reveal decodes base64 and then requires
strict, valid UTF-8. A missing value, malformed base64 transport, and valid
base64 containing non-UTF-8 bytes are distinct visible states. An empty byte
value is valid empty text and can be copied. The backend continues to support
arbitrary secret bytes, but this text UI cannot display or copy non-UTF-8 bytes
and does not provide a binary editor or download mode.

The libsecret item form accepts one `key=value` attribute per nonblank line. It
splits at the first `=`, trims both parts, ignores blank lines, permits empty
values, and treats keys as case-sensitive. A missing delimiter, empty trimmed
key, or repeated exact trimmed key is shown as a line-numbered field error and
no request is sent; duplicate errors identify the original line. Object-like
keys such as `__proto__` and `constructor` remain ordinary own string keys and
do not alter the attribute map's prototype. This is browser form validation,
not a backend/API rejection rule.

The delete dialog reports the complete blocker count across env entries, file
entries, libsecret aliases, collections (including empty collections), items,
wrapper/resource pairs, and permission rows. Typed content samples show only raw
identifiers and are escaped by React as text. They never include values, labels,
alias targets, attributes, content types, timestamps, or secret bytes. An
identifier can itself contain sensitive operator input, so the backend does not
copy samples into audit events or logs. A blocked response keeps the dialog open;
only a successful deletion closes it and refreshes the affected views.

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
deletion is blocked while stored resource content, pairs, or permission rows
depend on it. The UI reports those blockers so they can be removed in order.
After a successful resource deletion, the active empty content key and catalog
record are absent. This is not secure media erasure and does not remove retained
encrypted rotation generations.

A storage error can occur after an earlier deletion step became visible. The UI
does not report success in that case; re-read the resource and content state
before retrying after all relevant runtime writeback and admin content, pair,
permission, and deletion mutations have been serialized.

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

`UnsafeDebugUntil` gives `underwrap-debug-tool` the complete configured env,
files, and libsecret projection for the exact runtime client instance,
store, wrapper/resource pair, and expiry shown by the confirmation. The tool can
show an environment summary, start an interactive shell, or execute an arbitrary
command that can print, copy through accessible paths, or transmit the
projection. underwrap is not a behavioral sandbox.

The frontend instance entry is system-wide: [any local user can invoke
it](security.md#untrusted-caller) while the permission is active. Backend policy
authorizes a runtime client instance, wrapper, and resource, not a local Unix
user, and `allowedUsers` installs PATH shims only. Put sudo, polkit, or another
OS-level gate in front when only selected local users may invoke the wrapper or
a per-invocation prompt is required. The backend UI does not know the frontend
instance name; verify which deployment provides the conventional
`underwrap-<instance> debug <wrapper>` entry before saving.

With writable enabled, files/libsecret changes can be submitted and persisted
on successful finalization, affecting future invocations; env remains
backend-owned and immutable. With writable disabled, underwrap does not persist
staged changes, but full target access and disclosure are unchanged.

Selecting this mode proposes one exact local expiry using the 15-minute default.
Confirmation only copies that value into the form: the operator can review or
change it, and no backend permission becomes active until **Save**. The backend
rechecks expiry on every open and finalize. Use the shortest practical deadline
and return the permission to `Locked`, `Unlocked`, or `UnlockedUntil`
immediately afterward.

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
Sign-out always obtains a fresh synchronizer token before submitting logout,
including when `/logout` is opened as a fresh document. The UI clears its admin
cache and returns to `/login` only after the backend destroys the session. A
failed request remains visibly on `/logout` with a retry action; it does not
clear the cache or report sign-out success.

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
