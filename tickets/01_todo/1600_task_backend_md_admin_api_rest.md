# Task: Rewrite the `docs/backend.md` admin-API section to the real RESTful surface

Part of: docs accuracy follow-up (post-2026-05 epics).

## Problem

The admin API is fully RESTful and resource-scoped, but
`docs/backend.md` §"Admin API" still documents the **old RPC surface**.
This drift predates the single-instance/redesign epics (it is an
orthogonal RPC→REST refactor that landed without a doc update), so the
Phase 4 docs pass only fixed the `writes_enabled` lines in that section
and left the endpoint list wrong.

Real routes (from `crates/backend-backendd/src/admin_api/`):

```
/v1/admin/auth/{bootstrap,bootstrap/status,login,logout,password,whoami}
/v1/admin/users, /v1/admin/users/{id}, /v1/admin/users/{id}/password
/v1/admin/client-instances, /v1/admin/client-instances/{id}
/v1/admin/stores, /v1/admin/stores/{id}
/v1/admin/stores/{id}/{lock,unlock,rotate-key}
/v1/admin/stores/{id}/resources, /v1/admin/stores/{id}/resources/{rid}
/v1/admin/stores/{id}/pairs, /v1/admin/stores/{id}/pairs/{wrapper}/{rid}
/v1/admin/stores/{id}/pairs/with-new-resource
/v1/admin/stores/{id}/secrets, /v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}
/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}/rename
/v1/admin/stores/{id}/secrets-view
/v1/admin/stores/{id}/wrapper-permissions
/v1/admin/stores/{id}/wrapper-permissions/{client_instance}/{wrapper}/{rid}
/v1/admin/stores/{id}/wrapper-permissions-view
/v1/admin/stores/{sid}/resources/{rid}/libsecret/...
```

The doc still lists `POST /v1/admin/policy/{get,put}`,
`/v1/admin/secret/{read,write,list,delete}`,
`/v1/admin/wrapper-permissions/{list,upsert,delete}`,
`/v1/admin/resources/{list,create,delete}`,
`/v1/admin/pairs/{list,create,delete}` — none of which exist.

## What to do

- Rewrite `docs/backend.md` §"Admin API" to the routes above, grouped by
  resource, with the correct HTTP verbs (the route table in
  `admin_api/*.rs` and `openapi.json` are the source of truth).
- Note the auth model already documented elsewhere: session-cookie +
  synchronizer-token CSRF; no bearer token.
- Cross-check `docs/admin-ui.md` and any other doc that names admin
  endpoints; fix the same drift if present.
- Consider regenerating/excerpting from `openapi.json` so the list
  doesn't drift again.

## Acceptance

- Every endpoint named in `docs/backend.md` exists in the router.
- `grep -RnE "/v1/admin/(policy|secret|wrapper-permissions/(list|upsert|delete)|resources/(list|create|delete)|pairs/(list|create|delete))" docs/`
  returns nothing.
- No tests change (docs only).
