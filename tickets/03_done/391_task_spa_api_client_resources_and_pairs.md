# Task: SPA API client for Resources and Pairs

Parent: `390_story_spa_wrappers_page_and_secrets_resource_management.md`

## Scope

Replace `postCatalogGet` and the `CatalogEntry` TS type in
`web/src/api/client.ts` with a new resource/pair surface that mirrors
the backend endpoints from
`384_task_admin_crud_endpoints_for_resources_and_pairs.md`:

- `postResourcesList(store) → ResourceRecord[]`
- `postResourceCreate({ store, name }) → ResourceRecord`
- `postResourceDelete({ store, name }) → DeleteResult` (refusal shape)
- `postPairsList(store) → PairRecord[]`
- `postPairCreate({ store, wrapper, resource }) → PairRecord`
- `postPairDelete({ store, wrapper, resource }) → DeleteResult`

The generated TS types under `web/src/api/types/` come from ts-rs;
add the new ones and remove `CatalogEntry`,
`AdminCatalogGet{Request,Response}`, `ServiceCatalogPut{Request,Response}`.

`describeApiError` in `web/src/lib/error-message.ts` should grow
formatters for the new refusal shapes so the dialogs in tasks 392 and
393 can surface them without bespoke string work.

## Acceptance criteria

- [ ] All catalog API symbols deleted from `web/src/api/`.
- [ ] New helpers covered by a Vitest unit test asserting request body
      shape (mirrors the existing pattern in
      `web/src/api/client.test.ts`).
- [ ] `npm run typecheck` and `npm test` green in `web/`.

## Out of scope

- Page-level UI changes (392 / 393).
