# Task: SPA + Playwright coverage for resource/pair model

Parent: `390_story_spa_wrappers_page_and_secrets_resource_management.md`

## Scope

Cross-cutting test coverage for the SPA changes in story 390.

Vitest:

- Update `web/src/pages/secrets.test.tsx` to mock the new
  resources/pairs endpoints instead of catalog. Assert provenance is
  rendered, delete-refusal blockers are surfaced.
- Update / replace `web/src/pages/matrix.test.tsx` →
  `web/src/pages/wrappers.test.tsx`. Cover pair-create success +
  refusal, pair-delete success + grant-blocker refusal, provenance
  rendering.
- Add coverage on `web/src/pages/client-instances.test.tsx` for the
  auto-create checkbox.

Playwright (`web/e2e/`):

- Replace `web/e2e/matrix.spec.ts` with a `wrappers.spec.ts` that
  drives the new pair-create flow against a real backend fixture and
  asserts provenance appears in the row after creation.
- Extend `web/e2e/secrets.spec.ts` with a resource-create + delete
  round trip, including a refusal-path assertion when a secret still
  exists under the resource.
- Add a `web/e2e/auto-create-toggle.spec.ts` that flips the toggle
  on a fresh instance and verifies that the first wrapper request
  succeeds (creates entities) where it would otherwise be refused.

## Acceptance criteria

- [ ] `cd web && npm test` green.
- [ ] `cd web && npm run test:e2e` green inside `nix develop`.
- [ ] No test file still imports a catalog mock or
      `postCatalogGet`.
