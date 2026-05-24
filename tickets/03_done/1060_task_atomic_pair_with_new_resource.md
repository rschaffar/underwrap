# Task: Add `POST /v1/admin/stores/{id}/pairs/with-new-resource`

Resolves: S-6, S-9.

## Problem

The SPA's `CreatePairDialog` (`web/src/pages/wrapper-permissions/dialogs/create-pair.tsx:86-122`)
orchestrates "create new resource → check dup → create pair" as two
sequential backend calls. If pair creation fails after the resource
was created, the resource is orphaned silently.

The same dialog also runs a pre-flight `existingPairs.some(...)` check
(`:100-109`) before posting — this races with other operators and
duplicates a constraint the backend already enforces.

## What to do

1. Add a new admin endpoint `POST /v1/admin/stores/{id}/pairs/with-new-resource`
   that:
   - Takes the resource-create payload AND the wrapper to pair with.
   - Creates the resource and the pair in a single transactional
     write (both succeed or both fail).
   - Returns 409 with a stable error code when the
     `(wrapper, resource_id)` pair already exists.
2. Wire up the corresponding `Admin*Request`/`Response` types in
   `shared-protocol::service`.
3. Regenerate `schema.ts`.
4. In `CreatePairDialog`:
   - When the operator chose "create new resource", call the new
     endpoint instead of the two-step sequence.
   - Delete the `existingPairs.some(...)` pre-flight; let
     `describeApiError` map the backend's 409 error code to the
     friendly message.
   - Ensure `describeApiError` knows the stable error code returned
     by the backend; add a case if needed.
5. The "use existing resource" path can stay on the single pair-create
   endpoint.

## Acceptance

- `cargo test --workspace`
- `cargo test -p integration-tests` — add a test for the atomic
  endpoint covering: success, dup-pair conflict (409), invalid
  resource shape (rollback).
- `cd web && npm test && npm run build`
- Manual: create a pair with a new resource; verify the network tab
  shows one request, not two.
