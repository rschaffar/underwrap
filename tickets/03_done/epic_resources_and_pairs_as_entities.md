# Epic: Promote resources and wrapper/resource pairs to backend entities

## Background

Today the "catalog" is a per-store JSON blob at
`__underwrap/catalog/document` that holds `Vec<CatalogEntry>` with one
entry per `(client_instance, wrapper, resource)` tuple. Wrappers
re-publish their full local config to the backend on startup via
`POST /v1/catalog/put` (`crates/frontend-underwrapd/src/server.rs:47`),
overwriting whatever was there. Resources and wrapper/resource pairs are
not records anywhere — they only exist implicitly as strings inside
grants (`shared-protocol/src/policy.rs:62`) and catalog entries
(`shared-protocol/src/service.rs:153`). The SPA derives the "resource
universe" by unioning catalog entries with whatever the secret list
happens to contain.

That works, but it has three rough edges on the road to a polished
first release:

1. Resources and (wrapper, resource) pairs have no provenance. There is
   no answer to "who introduced this resource?" or "when did this pair
   first appear?".
2. The catalog blob is a wrapper-driven snapshot that gets rewritten in
   full on every daemon startup. Anything the wrapper drops from its
   local config silently vanishes from the catalog — there is no
   intentional admin act involved.
3. The "Matrix" page is mis-named for what it actually is: the
   management view for wrapper/resource pairs and their grants.

## Product decisions already made

- Resources become first-class backend entities with `created_by` and
  `created_at` provenance, write-once on create.
- Wrapper/resource pairs become first-class backend entities with the
  same provenance shape, referencing a resource by name.
- The runtime startup POST stays as a round-trip but switches from
  "publish the whole catalog" to "auto-create any missing resources and
  pairs". Provenance is set to the calling client instance.
- Auto-create is **opt-in per client instance**, off by default. An
  admin must enable it before that instance's wrappers can introduce
  new entities.
- Resource and pair deletion **refuses if any dependents exist**
  (secrets, pairs, grants). No force flag in this phase.
- The Matrix page is renamed to **Wrappers** and becomes the place to
  create wrapper/resource pairs by hand.
- Resource management stays on the **Secrets** page — "Create resource"
  now hits a real backend create, and provenance is shown inline.
- The `__underwrap/catalog/document` blob, `CatalogEntry` type, and the
  `/v1/catalog/put` + `/v1/admin/catalog/get` endpoints all go away.
- `client_instance` stays exactly where it is — part of the grant key,
  not promoted to anything else.
- Backward compatibility and migration helpers are not a constraint in
  this greenfield phase.

## Goals

1. Backend-persisted Resource records, one per store, with immutable
   `name`, `created_by`, `created_at`.
2. Backend-persisted Wrapper/Resource Pair records, one per
   `(wrapper, resource)`, with the same provenance shape.
3. Runtime startup endpoint that idempotently auto-creates missing
   resources and pairs, gated by a per-instance opt-in.
4. Admin CRUD for both entity types, with refuse-on-dependents delete.
5. SPA: Matrix → Wrappers rename, pair creation UI, resource creation
   wired to the backend, provenance visible to operators.
6. Catalog document, endpoints, and `register_catalog()` flow removed.

## Stories

- `380_story_backend_resource_and_pair_entities.md` — data model,
  runtime register, opt-in flag, admin CRUD, catalog removal, tests.
- `390_story_spa_wrappers_page_and_secrets_resource_management.md` —
  API client cutover, Matrix→Wrappers rename, Secrets page resource
  management, opt-in toggle UI, test coverage.

## Out of scope

- A separate `Wrappers` entity. Wrapper names remain plain strings that
  appear inside pair records and grants.
- Editing provenance after creation.
- Force-delete cascade for resources or pairs.
- Audit-log integration for entity creation beyond the provenance
  fields themselves.
