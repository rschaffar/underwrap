# Admin UI Next.js redesign RFC

Status: implemented, with the original multi-target UI model superseded.

This document records the first-release shape of the Next.js operator
console.

Because underwrap is still **greenfield**, the final design optimized for
clear trust boundaries and simpler operator UX rather than preserving the
initial route/config sketches.

## Final design

The operator UI fronts exactly **one** `underwrap-backendd` instance.

That means:

- no backend map in UI runtime config
- no target selector in the operator shell
- no `/t/[target]/...` route layer
- store becomes the top-level navigation scope inside one backend

This was a deliberate cleanup after implementation work exposed that the
old target abstraction belonged on the **client/runtime** side, not on the
backend admin side.

## Why the multi-target UI was removed

underwrap clients may indeed need credentials sourced from multiple
backends. But that concern lives in:

- `underwrapd`
- wrapper/runtime configuration
- service-client certificate mapping on the backend service path

It does **not** belong in the admin UI. Each backend has its own admin UI
instance and its own operator auth/session state.

## Current route map

- `/login`
- `/logout`
- `/`
- `/s/[store]/resources`
- `/s/[store]/resources/[...resource]`
- `/s/[store]/wrappers`
- `/s/[store]/audit`

## Current runtime YAML

```yaml
session_key_file: /run/keys/underwrap-admin-ui.session

operators:
  alice:
    password_hash_file: /etc/underwrap-admin-ui/alice.phc

backend:
  label: shame01
  admin_url: http://127.0.0.1:19443
  token_file: /run/keys/underwrap-admin-backend
```

`operators` may be empty/omitted for first-use bootstrap.

## Security posture

The final app keeps the originally intended trust boundary:

- browser never talks directly to `underwrap-backendd`
- backend bearer token stays server-side
- auth uses signed `HttpOnly` sessions
- state-changing routes enforce same-origin + CSRF checks
- reveal/download are explicit and `no-store`

## UI scope

The current console supports:

- operator login / bootstrap
- store overview from the dashboard
- resource browse + detail
- explicit reveal/download
- mutation-gated write/upload/delete
- wrapper mode management
- access-matrix toggles
- read-only audit history

## Historical note

Earlier drafts of this RFC described a multi-target dashboard and
`/t/[target]/...` routes. That design is obsolete and should not be used
for new implementation or deployment work.
