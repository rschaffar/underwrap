# Epic: Structured libsecret admin surface

## Background

Today the admin API projects the libsecret section of a resource through
`expand_blob_to_entries` into a fake flat key/value namespace
(`<rid>/libsecret/_aliases`, `<rid>/libsecret/<col>/_meta`,
`<rid>/libsecret/<col>/<item>.meta`, `<rid>/libsecret/<col>/<item>.secret`).
The admin SPA renders those rows generically as if libsecret were a normal
KV slot. This is wrong: libsecret is a small structured tree
(Service → Collections → Items, plus a Service-level alias table) and the
flat projection is purely a back-compat artifact that no runtime consumer
needs anymore.

Consequences operators see today:

- "Delete `_aliases`" silently re-synthesizes an empty `_aliases` row on
  the next read, because `libsecret_section_to_kv` re-emits `_aliases`
  whenever any collection exists. The audit log says `result=success`,
  but the operator sees the entry come back. This is a real bug filed
  during day-to-day use.
- Operators are exposed to the in-blob serialization shape
  (`_meta` / `.meta` / `.secret`) rather than the spec model (collection,
  item, alias). Deleting "an item" requires deleting two flat rows
  (`.meta` + `.secret`) and risks leaving orphans, which the bridge
  then silently scrubs on save.
- `_aliases`, which is a small JSON map of the whole alias table, is
  presented as if it were a single "secret entry" — there is no sensible
  per-row UI action for it.

The data model itself is fine. `LibsecretSection { aliases, collections }`
in `shared-protocol/src/resource_blob.rs` already covers full Secret
Service spec semantics (multi-collection, free-form alias→collection
mapping, per-item label / content_type / attributes / timestamps /
bytes). The runtime IPC paths (daemon ↔ backend, daemon ↔ secretsd)
already exchange the structured blob bytes verbatim.

Only the admin API and admin SPA still pretend the libsecret tree is a
flat KV slot.

## Goal

Expose libsecret to admin operators as the spec-faithful tree it actually
is: a Service-level alias table, a set of collections, and items inside
each collection. Each admin verb maps 1:1 to a spec concept; no
synthetic flat keys cross the admin API boundary.

`env` and `files` keep their existing flat-KV admin surface — they
really are key/value slots.

## End state

- `GET /v1/admin/stores/{sid}/resources/{rid}/libsecret` returns the
  structured tree (aliases, collections, items metadata; item secret
  bytes are fetched on a dedicated endpoint).
- Aliases, collections, and items each have their own structured
  create / update / delete endpoints.
- `expand_blob_to_entries` and `blob_from_resource_entries` no longer
  handle libsecret entries; their libsecret branches are removed, and
  the `secret_id` parsing layer at the admin boundary is narrowed to
  `env` / `files`.
- The admin SPA renders a real tree view and an aliases panel and no
  longer lists `_aliases` / `<col>/_meta` / `<item>.meta` /
  `<item>.secret` as rows.
- Audit events become spec-faithful (`libsecret.alias.set`,
  `libsecret.collection.delete`, `libsecret.item.set_secret`, etc.)
  instead of the opaque `secret.delete <flat-path>` shape.
- The original `_aliases` re-emission bug becomes structurally
  impossible because the synthetic `_aliases` key no longer exists in
  any code path.

## Decisions locked in by this epic

- Data model is **not** changing. `LibsecretSection { aliases,
  collections }` stays exactly as it is. This is a pure admin-surface
  rework.
- The collection layer **stays**. We are not collapsing libsecret into a
  single global collection per resource. A client that legitimately uses
  multiple collections must continue to see them as distinct.
- The alias table **stays**. Aliases can still map a name to any
  collection id, including names that differ from any collection id.
- The runtime path (daemon ↔ backend, daemon ↔ secretsd, secretsd D-Bus
  surface) is **not** touched by this epic.

## Stories

- `2200_story_libsecret_structured_admin_api.md` — add the new
  structured endpoints (tree GET, alias / collection / item CRUD),
  audit events, and integration tests.
- `2300_story_retire_flat_libsecret_bridge.md` — once the SPA is on the
  new endpoints, strip libsecret from `expand_blob_to_entries` /
  `blob_from_resource_entries`, drop the libsecret slot from the
  flat-KV admin endpoints, narrow `parse_secret_id` /
  `compose_secret_id` to env + files.
- `2400_story_libsecret_spa_tree_view.md` — admin SPA work (lives in the
  SPA tree under `$UNDERWRAP_WEB_DIST`; tracked here so the rollout
  order is visible).

## Out of scope

- Changing the persistent `ResourceBlob` schema in any way.
- Changing the daemon ↔ backend runtime API.
- Changing the daemon ↔ secretsd channel.
- Changing the D-Bus surface exposed by `frontend-secretsd`.
- Locked / unlocked semantics on collections or items. Underwrap's
  secretsd is always-unlocked by design.
- Master-password mechanisms, prompts, transport encryption between
  D-Bus client and secretsd (already plain; private bus).

## Rollout order

1. Land `2120_task_fix_aliases_re_emission_quickfix.md` first so the
   day-to-day operator bug is unblocked.
2. Land `2200_story_libsecret_structured_admin_api.md`. The new
   endpoints are additive; the flat endpoints still work.
3. SPA switches to the new endpoints (`2400`).
4. Land `2300_story_retire_flat_libsecret_bridge.md` to delete the now
   dead libsecret cases from the flat bridge.
