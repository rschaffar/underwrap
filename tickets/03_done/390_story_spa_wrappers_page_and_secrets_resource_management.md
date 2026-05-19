# Story: SPA cutover — Wrappers page, Secrets resource management, opt-in toggle

Parent: `epic_resources_and_pairs_as_entities.md`

## Outcome

After this story, the admin SPA no longer talks to the catalog API. The
Matrix page is renamed to **Wrappers** and gains a "Create wrapper/
resource pair" affordance. The Secrets page's "Create resource" button
hits the real backend; resource and pair rows show their `created_by`
and `created_at` provenance. The Client Instances page exposes an
**Auto-create wrapper assets** toggle that drives the runtime
register endpoint's gate.

## Tasks

- `391_task_spa_api_client_resources_and_pairs.md`
- `392_task_rename_matrix_to_wrappers_with_pair_management.md`
- `393_task_secrets_page_real_resource_management_with_provenance.md`
- `394_task_spa_per_instance_auto_create_toggle.md`
- `395_task_spa_and_e2e_test_coverage_for_entity_model.md`

## Acceptance criteria

- [ ] `web/src/api/client.ts` exposes resource/pair list/create/delete
      helpers; no `postCatalogGet` remains.
- [ ] `/matrix` route + page is gone, replaced by `/wrappers`. Nav,
      titles, and tests use the new name.
- [ ] Operators can create a wrapper/resource pair from the Wrappers
      page and a resource from the Secrets page; both surface refusal
      reasons returned by the backend (missing resource, dependents,
      duplicate, opt-in off).
- [ ] Provenance (`created_by`, `created_at`) is visible on resource
      and pair rows.
- [ ] Vitest + Playwright suites are green.
