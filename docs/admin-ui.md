# Admin UI

The underwrap admin UI is a Vite + React + shadcn single-page app that
ships embedded inside `underwrap-backendd`. There is no separate Node
or Next.js process — the SPA bundle is statically compiled into the
backend binary via `rust-embed` and served from the same HTTPS admin
listener that exposes the JSON admin API.

## Deployment shape

```text
browser ──HTTPS──► underwrap-backendd admin listener
                       │
                       ├── / and /assets/...      → SPA (embedded)
                       └── /v1/admin/...          → JSON admin API
```

The admin listener terminates TLS natively (rustls). No reverse proxy
is required, though one is still possible for centralised certificate
management.

## Authentication

- **Argon2id** password hashing.
- **AEAD-encrypted session cookies** signed and encrypted with a
  32-byte key configured via `admin.session_key_file`.
- **Double-submit CSRF** for every state-changing request: the SPA
  reads the `__Host-uw-csrf` cookie and echoes it back via the
  `X-CSRF-Token` header.
- **Same-origin checks** for state-changing requests via the
  `admin.allowed_origins` allowlist.
- **First-use bootstrap** flow that the SPA detects automatically via
  `GET /v1/admin/auth/bootstrap/status`.

A legacy bearer-token auth path remains in the backend for scripts and
the integration-test harness; new deployments should use the cookie
flow exclusively.

## Surface today

| Page                | Path        | Role required |
| ------------------- | ----------- | ------------- |
| Dashboard           | `/`         | any operator  |
| Wrapper permissions | `/wrappers` | any operator  |
| Stores              | `/stores`   | admin         |
| Users               | `/users`    | admin         |
| Profile             | `/profile`  | any operator  |
| Login               | `/login`    | unauthenticated |
| Bootstrap           | `/bootstrap`| empty backend |

Operators see only the stores they have been granted access to.
Admins see everything. The Wrapper permissions tab lets admins set
the `(client_instance, wrapper, resource)` permission mode and toggle
its `writable` flag — see [backend docs](backend.md#policy-model) for
the underlying model.

## Local development

Inside `nix develop`:

```bash
cargo build -p backend-backendd   # build the backend binary
cd web
npm install                       # first time only
npm run dev                       # Vite dev server with HMR (proxies to backendd)
```

For a production build:

```bash
cd web
npm run build                     # writes web/dist/
cargo build -p backend-backendd   # the embedded SPA picks up web/dist/
```

The dev server proxies `/v1/admin/*` to the backend, so the same
auth flows work without TLS. For Playwright/e2e runs see
`docs/testing.md`.
