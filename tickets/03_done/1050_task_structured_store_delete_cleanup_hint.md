# Task: Return structured `cleanup_hint` from store-delete

Resolves: S-5.

## Problem

The store-delete handler (`crates/backend-backendd/src/admin_api/stores/crud.rs`)
returns a freeform prose string for `cleanup_hint`. The SPA at
`web/src/pages/stores/dialogs/delete.tsx:36-43,67-69` dumps it verbatim
into a `<pre>`. UX quality of the message is now a backend concern
that the SPA cannot iterate on without backend changes.

## What to do

1. Change the wire shape so `cleanup_hint` is structured:
   ```rust
   pub struct AdminStoreDeleteCleanupHint {
       pub data_dir: PathBuf,
       pub command: String,
   }
   ```
   (Adjust field names as the data dictates — the goal is structured
   data, not a prose blob.) Update the matching `Admin*Response` DTO
   in `shared-protocol::service`.
2. Backend supplies the structured data; remove the prose formatting
   from `stores/crud.rs`.
3. Regenerate `schema.ts`.
4. In `web/src/pages/stores/dialogs/delete.tsx`, render the prose
   from the structured fields. Coordinate with `1340_task_web_dialog_shell_unification`
   which is also refactoring this dialog — easiest is to land 1050
   first and let 1340 consume the new shape.

## Acceptance

- `cargo test --workspace`
- `cargo test --test openapi_export`
- `cd web && npm test && npm run build`
- Manual: delete an empty store; the SPA shows the structured
  cleanup hint identically to the current prose.
