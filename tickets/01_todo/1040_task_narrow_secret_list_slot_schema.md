# Task: Narrow `slot` schema for `/v1/admin/stores/{id}/secrets`

Resolves: S-4 (and one entry from B-DOC-2).

## Problem

The secrets-list endpoint never emits `libsecret` entries, but the SPA
defensively filters them in `groupSecretEntries`:

- `web/src/pages/secrets/model.ts:9` declares
  `SECRET_SLOTS = ["env", "files"]`.
- `web/src/pages/secrets/model.ts:44-58` skips any other slot value
  with a comment about a "legacy flat-KV bridge" code path that no
  longer exists.
- `web/src/api/client.ts:516` carries a matching stale comment.

The runtime filter papers over a type imprecision in the OpenAPI
schema.

## What to do

1. In the backend, narrow the `slot` field type used by the secrets-
   list response to a non-libsecret subset. Two options:
   - Define a `KvSecretSlot` enum (`Env`, `Files`) in `shared-protocol`
     and use it on this endpoint's response DTO instead of the
     full `ResourceSlot`.
   - Or add `#[schema(enum = ["env", "files"])]` annotations so the
     generated schema's enum is narrower for this field.
   Pick whichever produces clean, idiomatic OpenAPI output.
2. Regenerate `schema.ts`. The narrowed TS union should make the
   defensive filter unreachable.
3. Delete the `SECRET_SLOTS` array and the runtime filter in
   `groupSecretEntries`; rewrite the function to assume well-typed
   input.
4. Delete the legacy-bridge comments in `web/src/pages/secrets/model.ts`
   and `web/src/api/client.ts:516`.

## Acceptance

- `cargo test -p backend-backendd`
- `cargo test --test openapi_export`
- `cd web && npm test && npm run build`
- The regenerated TS type for the slot field has exactly the values
  `"env" | "files"` (no `"libsecret"`).
