# Admin UI

The underwrap admin UI is a Vite + React + shadcn single-page app that
ships embedded inside `underwrap-backend`. There is no separate Node
or Next.js process — the SPA bundle is statically compiled into the
backend binary via `rust-embed` and served from the same HTTPS admin
listener that exposes the JSON admin API.

## Deployment shape

```text
browser ──HTTPS──► underwrap-backend admin listener
                       │
                       ├── / and /assets/...      → SPA (embedded)
                       └── /v1/admin/...          → JSON admin API
```

The admin listener terminates TLS natively (rustls). No reverse proxy
is required, though one is still possible for centralised certificate
management.

## Authentication

- **Argon2id** password hashing.
- **Opaque session cookies** issued by `tower-sessions` (`__Host-underwrap-session`,
  `Secure; HttpOnly; SameSite=Strict; Path=/`) backed by an in-memory
  store. Sessions do not survive a daemon restart — operators
  re-authenticate on reboot.
- **Synchronizer-token CSRF** via `axum_csrf`: the backend issues an
  HMAC-bound token in the `__Host-underwrap-csrf` cookie and the matching
  `X-CSRF-Token` response header. The SPA picks the token off the
  response header and echoes it back as `X-CSRF-Token` on every
  state-changing request.
- **Per-username login rate limit** (5 attempts / 60 s) plus a
  constant-time Argon2 verify against a dummy hash on unknown
  usernames to prevent enumeration.
- **Server-side session revocation on password rotation / user
  deletion**: `axum-login` stores the password-hash bytes in each
  session and re-checks them on every request via
  `session_auth_hash()`, so rotating a password or deleting a user
  invalidates every existing session for that operator on the next
  request — no separate revocation table required.
- **Same-origin checks** for state-changing requests via the
  `admin.allowed_origins` allowlist.
- **First-use bootstrap** flow that the SPA detects automatically via
  `GET /v1/admin/auth/bootstrap/status`.

## Surface today

| Page                | Path               | Visible to      |
| ------------------- | ------------------ | --------------- |
| Wrapper permissions | `/wrappers`        | any user        |
| Secrets             | `/secrets`         | any user        |
| Stores              | `/stores`          | any user        |
| Client instances    | `/client-instances`| any user        |
| Users               | `/users`           | any user        |
| Profile             | `/profile`         | any user        |
| Login               | `/login`           | unauthenticated |
| Bootstrap           | `/bootstrap`       | empty backend   |

`/` is not a page of its own — it redirects to `/secrets`.

Authorisation is flat: any authenticated user can perform any admin
action. The Wrapper permissions tab sets the
`(client_instance, wrapper, resource)` permission mode and toggles its
`writable` flag — see [backend docs](backend.md#policy-model) for the
underlying model.

## Local development

Inside `nix develop`:

```bash
cargo build -p underwrap-backend   # build the backend binary
cd web
npm install                       # first time only
npm run dev                       # Vite dev server with HMR (proxies to backend)
```

For a production build:

```bash
cd web
npm run build                     # writes web/dist/
cargo build -p underwrap-backend   # the embedded SPA picks up web/dist/
```

The dev server proxies `/v1/admin/*` to the backend, so the same
auth flows work without TLS. For Playwright/e2e runs see
`docs/testing.md`.
