# Task: Docs cutover for the resource/pair entity model

Parent: `epic_resources_and_pairs_as_entities.md`

## Scope

Bring the project docs in line with the new entity model after the
backend and SPA changes have landed.

Files to touch:

- `docs/backend.md` — replace the "Catalog" section with **Resources**
  and **Wrapper/Resource Pairs** sections. Cover the data model,
  provenance, runtime auto-create endpoint and its opt-in gate, and
  admin CRUD with delete-refusal behavior.
- `docs/security.md` — update the "Design invariants" section if the
  catalog is mentioned; otherwise note that wrapper-driven creation
  requires per-instance opt-in.
- `CLAUDE.md` — replace any mention of catalog (key files,
  architecture summary, grant-key paragraph) with the new entities.
  Mention that the Matrix page is now Wrappers and that resources are
  managed on the Secrets page.
- `examples/config.yaml` — comment update only; the YAML still
  declares wrapper resources, but the note explaining what the
  daemon does with them must reflect the new auto-create-on-startup
  semantics.

## Acceptance criteria

- [ ] No doc references the catalog as a live concept; historical
      mentions, if any, are explicitly marked as legacy.
- [ ] Runtime register endpoint, its request/response shape, and the
      opt-in gate are documented in `docs/backend.md`.
- [ ] CLAUDE.md "Key files to understand first" list is accurate
      against the new tree.
