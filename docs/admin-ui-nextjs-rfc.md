# Admin UI Next.js redesign RFC

Status: proposed implementation plan for the first-release admin console.

This document defines the target shape for replacing the current Rust
HTML admin UI with a modern Next.js operator console.

Because underwrap is still **greenfield**, this RFC optimizes for a
strong first release, not backwards compatibility with the current UI.
The existing `crates/backend-admin-ui` implementation is treated as a
replaceable stopgap.

## Goals

The new admin UI should feel like a finished operator product, not a set
of stitched-together forms.

Primary goals:

1. **Modern operator UX** for resources, wrapper state, access policy,
   mutation windows, and audit history.
2. **Server-side trust boundary**: the browser never talks directly to
   `underwrap-backendd` and never receives backend bearer tokens.
3. **Deliberate secret handling**: reveal is explicit, masked by
   default, and auditable.
4. **Real file workflows** for file-backed secrets: upload and download,
   not textarea-only editing.
5. **First-class multi-target support** for operators who manage more
   than one backend target.
6. **First-class NixOS deployment shape**.

## Non-goals

This RFC does **not** include:

- OIDC / SSO
- RBAC redesign
- mobile-first design
- preserving the current Rust UI structure or routes for compatibility
- direct browser access to backend admin endpoints

## High-level architecture

```text
browser
  -> TLS reverse proxy
  -> underwrap-admin-ui (Next.js server / BFF)
  -> underwrap-backendd admin API
```

The Next.js app is the only browser-facing service. It performs all
backend calls server-side and holds:

- session signing key
- operator password-hash files
- backend target bearer tokens

The browser receives only:

- rendered HTML / JS / CSS
- same-origin app requests
- signed session cookie
- CSRF token / same-origin request protections as needed for mutations

### Why this shape

This keeps the current good security property from the Rust UI:
**backend admin tokens never enter the browser**.

## UI implementation posture

The target implementation is:

- **Next.js App Router**
- **TypeScript**
- **server-rendered by default**
- **Route Handlers** for file upload/download and other explicit HTTP
  surfaces
- **Server Actions** only for small same-origin mutations where the
  abstraction stays clear

Notes:

- Next.js Server Actions default to a **1 MiB** request-body limit. That
  matches the v1 product limit for file-backed secrets.
- Even with that match, file upload/download should go through explicit
  Route Handlers so size checks, MIME handling, audit behavior, and
  error handling remain obvious and testable.

## UX principles

1. **Task-oriented, not schema-oriented**
   - operators should manage resources, wrappers, mutation windows, and
     audit history directly
   - they should not need to understand raw policy-document structure
     for normal workflows

2. **Material-sensitive display**
   - metadata-first lists
   - secret material only after explicit reveal/download action

3. **Dark, high-contrast ops console**
   - good legibility on dense tables and log-like views
   - strong state colors for locked / unlocked / expiring / denied

4. **Multi-target always visible**
   - switching backend target is a first-class workflow, not a buried
     settings page

5. **Auditability over magic**
   - reveal/download/mutation flows are explicit and generate backend
     audit events

## Information architecture

### Route map

The Next.js app should expose these primary routes:

- `/login`
  - username/password sign-in
- `/logout`
  - explicit logout endpoint/action
- `/`
  - multi-target landing page / dashboard
- `/t/[target]`
  - target overview
- `/t/[target]/s/[store]/resources`
  - resource index with search/filter
- `/t/[target]/s/[store]/resources/[...resource]`
  - resource detail
- `/t/[target]/s/[store]/wrappers`
  - wrapper console
- `/t/[target]/s/[store]/audit`
  - read-only audit history

### Persistent navigation

The authenticated shell should keep these controls visible:

- **target switcher**
- **store switcher** for the selected target
- primary nav:
  - Overview
  - Resources
  - Wrappers
  - Audit
- operator menu / logout

## Page definitions

### 1. Landing page: `/`

Purpose:

- show configured backend targets
- surface high-level counts/status per target
- let the operator jump directly into a target

Cards per target should include at least:

- target name
- reachable/unreachable status
- store count
- wrapper count summary
- active mutation-window count
- recent audit activity summary

### 2. Target overview: `/t/[target]`

Purpose:

- orient the operator before they drill into a store
- summarize each store in that target

Per-store summary should include:

- resource count
- wrapper count
- locked/unlocked/per-call counts
- active mutation-window count
- recent audit count / recent sensitive actions

### 3. Resource index: `/t/[target]/s/[store]/resources`

Purpose:

- searchable resource inventory
- jump-off point for resource detail work

The index should support filtering by at least:

- resource id / prefix
- wrapper presence
- slot presence (`env`, `files`, `libsecret`)
- mutation-window state

Each row should show:

- resource id
- which slots have stored entries
- wrapper/catalog presence
- mutation state summary
- last relevant audit timestamp if available

### 4. Resource detail: `/t/[target]/s/[store]/resources/[...resource]`

Purpose:

- one place to inspect and manage a resource

The page should have three slot sections/tabs:

- **env**
- **files**
- **libsecret**

And supporting side panels/sections for:

- mutation-window state
- access matrix filtered to the current resource
- recent audit entries for the current resource

#### env slot UX

Each env entry shows:

- variable name
- masked value preview
- reveal action
- copy action after reveal
- delete action
- write/update action

#### files slot UX

Each file entry shows:

- relative path
- byte size
- text/binary classification if known
- preview action when UTF-8 text
- download action
- replace/upload action
- delete action

#### libsecret slot UX

Each item shows:

- item id / relative id
- masked material preview
- reveal action
- copy action after reveal
- delete action
- write/update action

### 5. Wrapper console: `/t/[target]/s/[store]/wrappers`

Purpose:

- manage wrapper modes
- inspect wrapper-related access policy state

The primary table should support filtering by:

- wrapper name
- mode
- resource
- host
- user

Each wrapper row should show:

- wrapper name
- current mode
- reason / note
- expiration if applicable
- related resource count

The page should support:

- lock
- unlock permanently
- unlock until datetime
- per-call approval with public key

### 6. Audit history: `/t/[target]/s/[store]/audit`

Purpose:

- read-only operator inspection of security-relevant events

Filters:

- target (shell-level)
- store
- operator
- wrapper
- resource
- action
- time range

Important action families include at least:

- secret reveal
- secret download
- secret write
- secret delete
- mutation enable / disable
- wrapper mode change
- access-matrix change
- deny events relevant to operators

## Primary operator workflows

### Workflow A: inspect and reveal an env secret

1. choose target
2. choose store
3. search resource
4. open resource detail
5. click reveal on one env entry
6. backend records explicit reveal audit event
7. operator may copy after reveal

### Workflow B: replace a file-backed secret

1. choose target/store/resource
2. inspect mutation state
3. enable mutation window for the relevant wrapper/resource pair if needed
4. upload replacement file (<= 1 MiB)
5. backend stores new material and audits the write
6. operator downloads or previews to verify if desired
7. operator disables mutation or lets it expire

### Workflow C: lock down or unlock a wrapper

1. choose target/store
2. open wrapper console
3. filter/select wrapper
4. change mode
5. confirm visible state change in the table
6. backend audits the mode change

### Workflow D: inspect what happened

1. choose target/store
2. open audit page
3. filter by resource / wrapper / operator / action / time range
4. inspect reveal/download/write/mode-change events

## Backend API design

### Namespace

Add a dedicated frontend-oriented namespace:

- `GET/POST /v1/admin/ui/*`

This keeps the task-oriented UI contract separate from the existing
low-level admin primitives.

The current low-level endpoints remain useful implementation building
blocks, but the Next.js app should not need to assemble page models from
raw policy documents plus secret-id lists plus catalog records.

## Request context

The backend **does not** know the UI target name. The target is the
Next.js app's selection of which backend config entry to call.

Therefore:

- the UI route contains `[target]`
- backend API payloads contain **store**, but **not target**
- the Next.js server chooses the backend URL/token based on the target

## Common response rules

Unless otherwise noted:

- responses are JSON
- read-model responses contain **metadata only** by default
- secret material appears only in explicit reveal/download responses
- all state-changing endpoints are auditable
- all reveal/download endpoints are auditable

## Proposed read-model endpoints

### `GET /v1/admin/ui/overview`

Purpose:

- provide shell/dashboard data for one backend target

Response shape:

```json
{
  "stores": [
    {
      "store": "work",
      "resource_count": 128,
      "wrapper_counts": {
        "locked": 2,
        "unlocked": 14,
        "unlocked_until": 1,
        "per_call_approval": 3
      },
      "active_mutation_windows": 4,
      "recent_audit_events": 18
    }
  ]
}
```

### `POST /v1/admin/ui/resources/query`

Purpose:

- resource listing/search for one store

Request shape:

```json
{
  "store": "work",
  "query": "gh/",
  "slot": null,
  "mutation_state": "any",
  "page": 1,
  "page_size": 50
}
```

Response shape:

```json
{
  "items": [
    {
      "resource": "gh/main",
      "slots": {
        "env": true,
        "files": false,
        "libsecret": true
      },
      "catalog_wrappers": ["gh-authorized"],
      "mutation": {
        "active": true,
        "wrappers": [
          {
            "wrapper": "gh-authorized",
            "expires_at_epoch": 1760000000
          }
        ]
      },
      "last_audit_at_epoch": 1760000100
    }
  ],
  "total": 1
}
```

### `POST /v1/admin/ui/resource/get`

Purpose:

- full resource detail without returning secret material

Request shape:

```json
{
  "store": "work",
  "resource": "gh/main"
}
```

Response shape:

```json
{
  "resource": "gh/main",
  "slots": {
    "env": [
      {
        "entry": "GH_TOKEN",
        "secret_id": "gh/main/env/GH_TOKEN",
        "size_bytes": 40,
        "is_utf8": true
      }
    ],
    "files": [
      {
        "entry": "hosts.json",
        "secret_id": "gh/main/files/hosts.json",
        "size_bytes": 812,
        "is_utf8": true,
        "sha256_hex": "..."
      }
    ],
    "libsecret": [
      {
        "entry": "default/item_1",
        "secret_id": "gh/main/libsecret/default/item_1",
        "size_bytes": 384,
        "is_utf8": false
      }
    ]
  },
  "mutation": {
    "wrappers": [
      {
        "wrapper": "gh-authorized",
        "active": true,
        "expires_at_epoch": 1760000000
      }
    ]
  },
  "access_matrix_summary": {
    "rows": 2,
    "allowed_cells": 4,
    "denied_cells": 0
  },
  "recent_audit": [
    {
      "ts_epoch": 1760000100,
      "action": "secret_revealed",
      "operator": "alice"
    }
  ]
}
```

### `POST /v1/admin/ui/resource/reveal`

Purpose:

- explicit reveal for env / files / libsecret entries
- returns a representation suitable for inline display/copy
- emits an explicit backend audit event

Request shape:

```json
{
  "store": "work",
  "secret_id": "gh/main/env/GH_TOKEN"
}
```

Response shape for UTF-8 material:

```json
{
  "kind": "utf8",
  "text": "ghp_xxx",
  "size_bytes": 40,
  "copy_allowed": true
}
```

Response shape for non-UTF-8 material:

```json
{
  "kind": "binary",
  "base64": "AAECAw==",
  "size_bytes": 4,
  "copy_allowed": false,
  "download_recommended": true
}
```

Notes:

- reveal responses must use `Cache-Control: no-store`
- the audit event records operator, store, secret id, and action, but
  never secret material
- for file-backed secrets, reveal is intended for inline preview only
  when the payload is valid UTF-8 and suitable for preview

### `POST /v1/admin/ui/resource/download`

Purpose:

- explicit raw download path, primarily for file-backed secrets
- emits an explicit backend audit event

Request shape:

```json
{
  "store": "work",
  "secret_id": "gh/main/files/hosts.json"
}
```

Response shape:

- HTTP 200 with raw bytes body
- headers:
  - `Content-Type: application/octet-stream`
  - `Content-Disposition: attachment; filename="hosts.json"`
  - `Cache-Control: no-store`

### `POST /v1/admin/ui/resource/write-text`

Purpose:

- write env or libsecret text-oriented entries
- file-backed secrets should use the upload-specific endpoint below

Request shape:

```json
{
  "store": "work",
  "wrapper": "gh-authorized",
  "secret_id": "gh/main/env/GH_TOKEN",
  "material_utf8": "ghp_new"
}
```

Notes:

- this remains subject to `store.write` mutation gating
- denial should return a structured authorization error the UI can turn
  into a mutation-window prompt

### `POST /v1/admin/ui/resource/upload-file`

Purpose:

- upload or replace a file-backed secret
- real file upload path for v1
- explicit 1 MiB limit

Transport:

- browser -> Next.js Route Handler: `multipart/form-data`
- Next.js Route Handler -> backend: either raw multipart proxying or
  server-side conversion into the backend's chosen write contract
- backend contract for this endpoint may be `multipart/form-data` or a
  byte body plus metadata headers; the key requirement is that the
  backend enforces the same 1 MiB limit and audits the write

Minimum logical fields:

- `store`
- `wrapper`
- `secret_id`
- `filename`
- file bytes

Behavior:

- reject payloads over **1 MiB** in v1
- write remains subject to mutation gating
- success returns updated entry metadata

### `POST /v1/admin/ui/resource/delete-entry`

Purpose:

- delete one stored entry

Request shape:

```json
{
  "store": "work",
  "secret_id": "gh/main/files/hosts.json"
}
```

## Mutation-window endpoints

### `POST /v1/admin/ui/mutation/get`

Purpose:

- return the mutation state for one resource

Request shape:

```json
{
  "store": "work",
  "resource": "gh/main"
}
```

Response shape:

```json
{
  "wrappers": [
    {
      "wrapper": "gh-authorized",
      "active": true,
      "expires_at_epoch": 1760000000,
      "mode": "temporary"
    }
  ]
}
```

### `POST /v1/admin/ui/mutation/set`

Purpose:

- enable or disable a mutation window without forcing the Next.js app to
  compose raw grants itself

Request shape:

```json
{
  "store": "work",
  "wrapper": "gh-authorized",
  "resource": "gh/main",
  "mode": "temporary",
  "expires_at_epoch": 1760000000
}
```

Alternative disable request:

```json
{
  "store": "work",
  "wrapper": "gh-authorized",
  "resource": "gh/main",
  "mode": "disabled"
}
```

## Wrapper and matrix endpoints

### `POST /v1/admin/ui/wrappers/query`

Purpose:

- wrapper table data for one store

Request shape:

```json
{
  "store": "work",
  "query": "gh",
  "mode": "any"
}
```

Response shape:

```json
{
  "items": [
    {
      "wrapper": "gh-authorized",
      "mode": {
        "kind": "per_call_approval",
        "until_epoch": null
      },
      "reason": "ops approval required",
      "resource_count": 2
    }
  ]
}
```

### `POST /v1/admin/ui/wrapper/set-mode`

Purpose:

- task-oriented wrapper mode mutation for the UI

Request shape:

```json
{
  "store": "work",
  "wrapper": "gh-authorized",
  "mode": {
    "kind": "unlocked",
    "until_epoch": 1760000000
  },
  "reason": "maintenance window"
}
```

### `POST /v1/admin/ui/matrix/query`

Purpose:

- matrix rows for a store or for one resource detail page

Request shape:

```json
{
  "store": "work",
  "resource": "gh/main",
  "host": null,
  "user": null,
  "wrapper": null
}
```

Response shape:

```json
{
  "operations": [
    "env.read",
    "files.read",
    "files.write",
    "libsecret.read",
    "libsecret.write"
  ],
  "rows": [
    {
      "host": "deepthought",
      "user": "robert",
      "wrapper": "gh-authorized",
      "cells": {
        "env.read": true,
        "files.read": false,
        "files.write": false,
        "libsecret.read": true,
        "libsecret.write": false
      }
    }
  ]
}
```

### `POST /v1/admin/ui/matrix/set-cell`

Purpose:

- add or remove one allow cell without making the UI handcraft low-level
  rule ids

Request shape:

```json
{
  "store": "work",
  "host": "deepthought",
  "user": "robert",
  "wrapper": "gh-authorized",
  "resource": "gh/main",
  "operation": "env.read",
  "enabled": true
}
```

## Audit endpoints

### `POST /v1/admin/ui/audit/query`

Purpose:

- read-only audit history feed for the operator UI

Request shape:

```json
{
  "store": "work",
  "operator": "alice",
  "wrapper": null,
  "resource": "gh/main",
  "action": "secret_revealed",
  "from_epoch": 1759990000,
  "to_epoch": 1760001000,
  "page": 1,
  "page_size": 100
}
```

Response shape:

```json
{
  "items": [
    {
      "ts_epoch": 1760000100,
      "operator": "alice",
      "action": "secret_revealed",
      "store": "work",
      "wrapper": null,
      "resource": "gh/main",
      "secret_id": "gh/main/env/GH_TOKEN",
      "result": "allowed"
    }
  ],
  "total": 1
}
```

### Required new audit actions

The backend should emit explicit UI-driven audit actions for at least:

- `secret_revealed`
- `secret_downloaded`
- `secret_uploaded`
- `secret_deleted`
- `mutation_enabled`
- `mutation_disabled`
- `wrapper_mode_changed`
- `matrix_cell_enabled`
- `matrix_cell_disabled`

Names can still be revised during implementation, but the distinction
between reveal and download must remain explicit.

## Secret-material semantics

### Metadata-first listings

Resource lists and detail pages return metadata only:

- entry name
- size
- text/binary classification if known
- digest if useful
- timestamps / audit summaries

They do **not** return secret material.

### Reveal semantics

Reveal is for **inline inspection**.

- default state: masked
- explicit operator action required
- response is never cacheable
- response is auditable
- UTF-8 material returns text
- non-UTF-8 material returns base64 + metadata, not lossy text

### Copy semantics

Copy is a UI affordance layered on top of successful reveal.

- env/libsecret UTF-8 values may be copied directly
- binary responses should not pretend to be human-readable text

### Download semantics

Download is for **raw material retrieval**, especially file-backed
secrets.

- explicit operator action required
- never cacheable
- auditable as a distinct action from reveal
- preferred path for binary file content

### Upload semantics

Upload is for file-backed secrets only in v1.

- real file picker / multipart form UX
- hard limit: **1 MiB** per uploaded file
- backend re-checks the limit; the browser-side limit is not trusted
- write remains gated by mutation policy

## Error-handling contract

The UI-oriented backend endpoints should prefer structured errors over
opaque strings. In particular, the frontend needs to distinguish:

- unauthenticated / bad bearer token
- unknown store / resource / wrapper
- mutation gate denied
- file too large (> 1 MiB)
- unsupported reveal form for the requested content
- audit/history filter validation errors

Suggested shape:

```json
{
  "code": "mutation_denied",
  "message": "store.write not currently allowed for wrapper gh-authorized on resource gh/main"
}
```

## Why this is better than the current UI contract

The current Rust UI effectively does this:

- list stores
- fetch raw policy document
- fetch catalog
- fetch secret-id list
- derive page models in the UI layer

That is workable for a form-based app, but it produces fragile page
composition and couples UI implementation to backend storage layout.

The proposed contract instead gives the UI:

- resource index models
- resource detail models
- explicit reveal/download behavior
- explicit mutation models
- wrapper table models
- matrix models
- audit feed models

This is a better first-release architecture.

## Implementation follow-ons

This RFC should drive the next tickets:

- scaffold the Next.js app and shell
- add the `/v1/admin/ui/*` backend endpoints
- implement auth/session/CSRF in the new app
- implement resource reveal/upload/download flows
- implement wrapper and matrix pages
- implement audit history page
- document NixOS deployment and remove the old Rust UI
