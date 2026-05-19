# Task: Secrets page — real resource records + provenance + delete

Parent: `390_story_spa_wrappers_page_and_secrets_resource_management.md`

## Scope

Wire `web/src/pages/secrets.tsx` to the new Resource backend
collection. Today the page unions secret IDs, catalog entries, and a
local `manualResources` React state to figure out which resource
blocks to render (`secrets.tsx:198-222`). After this task, that union
becomes:

- `postResourcesList(store)` → authoritative list of resources for
  this store.
- Secret IDs that don't match any known resource fall into the
  existing "Unrecognized layout" block — unchanged.
- `manualResources` and the empty-state-only logic for it disappear.

UI changes:

- **Create resource** dialog now hits `postResourceCreate`. Surface
  refusal on duplicate name with a clear message. On success, the new
  resource block appears immediately (invalidate the query).
- Each resource block header shows provenance: `created_by` (linked to
  the operator or client-instance detail where appropriate) and a
  relative `created_at` with a tooltip for the full ISO timestamp.
- A new **Delete resource** action on each block, behind the new
  simple confirm/cancel dialog (no type-to-confirm). The dialog calls
  `postResourceDelete`; on a refusal response, it stays open and lists
  the blocking dependents (secret count, pair count, grant count)
  with a hint to clear those first.

Reserved-prefix filtering (`__underwrap/...`) keeps applying so the
new entity documents stay hidden from the secret list.

## Acceptance criteria

- [ ] No reference to `manualResources` or `postCatalogGet` in
      `secrets.tsx`.
- [ ] Create resource is persisted across reload.
- [ ] Provenance visible on each resource block.
- [ ] Delete resource: success path empties the block + removes it;
      refusal path keeps the dialog open with a structured blocker
      list.
- [ ] Vitest unit tests for the page green (rewritten where they used
      catalog mocks).

## Out of scope

- E2E coverage — covered by 395.
