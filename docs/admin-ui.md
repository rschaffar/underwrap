# Admin UI

`underwrap-admin-ui` is the supported operator-facing web service for the
central backend.

The implementation is the Next.js app in `apps/admin-ui/`. It acts as a
server-side BFF in front of `underwrap-backendd`:

- the browser never talks to `underwrap-backendd` directly
- backend bearer tokens remain server-side only
- operator auth uses Argon2 password hashes, signed `HttpOnly` sessions,
  same-origin checks, and CSRF validation on state-changing routes

## Deployment shape

```text
browser ──TLS──► reverse proxy ──HTTP──► underwrap-admin-ui ──HTTP bearer──► underwrap-backendd admin
```

The app itself serves plain HTTP. Put it behind a TLS-terminating reverse proxy
when exposed beyond localhost.

## Runtime configuration

The Next.js server reads its runtime config from `UNDERWRAP_ADMIN_UI_CONFIG`.
That YAML file contains:

```yaml
session_key_file: /run/keys/underwrap-admin-ui.session

operators:
  alice:
    password_hash_file: /etc/underwrap-admin-ui/alice.phc
  bob:
    password_hash_file: /etc/underwrap-admin-ui/bob.phc

backends:
  corp:
    admin_url: http://127.0.0.1:19443
    token_file: /run/keys/underwrap-admin-corp
  personal:
    admin_url: http://127.0.0.1:19444
    token_file: /run/keys/underwrap-admin-personal
```

- `session_key_file` must contain exactly 32 random bytes
- each `password_hash_file` must contain an Argon2 PHC string
- each `backends.*.token_file` must match a configured
  `underwrap-backendd` admin bearer token

The server binds through the normal Next.js environment variables:

- `HOSTNAME`
- `PORT`
- `UNDERWRAP_ADMIN_UI_CONFIG`

## NixOS deployment

Use the flake's admin UI module export:

```nix
{
  imports = [ underwrap.nixosModules.admin-ui ];

  services.underwrap-admin-ui = {
    enable = true;
    package = underwrap.packages.${pkgs.system}.underwrap-admin-ui;
    configFile = /run/keys/underwrap-admin-ui.yaml;
    listenAddress = "127.0.0.1";
    port = 3000;
  };
}
```

The module wires:

- the `underwrap-admin-ui` systemd service
- `HOSTNAME`, `PORT`, and `UNDERWRAP_ADMIN_UI_CONFIG`
- the packaged standalone Next.js server

## Main operator workflows

- log in as an operator
- choose backend target and store
- browse resources
- explicitly reveal or download secret material
- enable mutation, write/upload/delete, then revoke mutation
- manage wrapper modes
- toggle access-matrix allow cells
- inspect the read-only audit history feed

## Security notes

- secret values are never placed in URLs
- reveal/download responses are `Cache-Control: no-store`
- secret values and backend tokens are not written to browser storage
- audit remains backend-owned; the UI only reads the backend feed

## Testing

Frontend checks live in `apps/admin-ui/`:

```bash
cd apps/admin-ui
npm test
npm run build
npm run test:e2e
```

The browser regression suite uses Playwright and covers:

- protected-route/session failures
- CSRF rejection
- reveal/download no-store behavior
- mutation, upload, delete, wrapper-mode, matrix, and audit flows
