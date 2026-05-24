# Refinement review: web SPA + backend-backendd + admin-API seam

Date: 2026-05-24
Reviewer session: claude-opus-4-7 (1M context)

## Scope

The user asked for a sweeping single-report review of:

- `crates/backend-backendd/` (every Rust file, production code only)
- `web/src/` (every TS/TSX file under `pages/`, `hooks/`, `lib/`, `components/`,
  `api/`; excluding `api/schema.ts` which is openapi-typescript output and the
  shadcn-derived `components/ui/*` boilerplate)
- The seam between them: the admin OpenAPI surface, the SPA's `api/client.ts`
  wrapper layer, and the wire shapes the two share via `shared-protocol::service`.

Per the user's standing direction ("don't care about churn, do care about a
clean simple product") the review is greenfield-aggressive: wire-format
renames, NixOS module changes, and `pub` tightening are all on the table.

## Summary

**111 findings** across 10 categories.

Severity split:

- **Must-fix:** 10 — duplication that is actively misleading or producing
  inconsistent behaviour today, dead `pub` surface that escapes the crate,
  one silent-failure bug in policy cleanup, the `storeName`/`storeId`
  naming bug that has already leaked into multiple files.
- **Nice-to-have:** 101 — simplifications, dead helpers, awkward
  abstractions, doc/comment hygiene, single-caller wrappers.

Category counts (descending):

| Count | Category |
|------:|----------|
| 31 | simplification |
| 21 | duplication |
| 17 | dead code / API hygiene |
| 12 | complexity |
| 10 | boundaries / size |
|  7 | doc / comment hygiene |
|  6 | naming |
|  4 | errors |
|  2 | logging |
|  1 | greenfield cleanup |

**Project-wide sweep results** (all clean):

- Inline `mod tests {}` blocks: **none**
- `#[cfg(test)] pub(crate) use` re-exports at crate roots: **none**
- `tests.rs` files over 600 lines: **none** (largest is 421)
- Ticket references in code/config/docs: **none**
- Migration / old-version support (`V1`, `Legacy`, `migrate_from`, …): **none**
- `serde(default)` audit: every backend hit reviewed; all are present-tense
  defaults, not legacy tolerance (see findings B-CFG-1/2/3 for the ones that
  are nonetheless redundant or wrongly typed).

The repo's convention discipline is good. Every finding below is about the
shape of the code itself, not about violated conventions.

**Proposed ticket shape:** **one epic + 18 child task tickets**.
Volume is high but the findings group cleanly into thematic bundles; a flat
list of 100+ tickets would be unmanageable. See the "Ticket plan" section
at the bottom.

## Cross-cutting themes

1. **Two response shapes inside one HTTP listener** — admin uses
   `IntoResponse` + `AdminError`, runtime uses raw `hyper::Response<Full<Bytes>>`
   with hand-built error envelopes. This forces ~30 lines of `match
   load_…(store) { Ok(d) => d, Err(e) => return error_response(…) }`
   boilerplate per runtime handler. Consolidation is finding B-RT-1.

2. **Three near-clone on-disk-registry modules** — `users.rs`,
   `store_registry.rs`, `runtime_clients.rs` each carry the same
   load/save/delete/validate scaffolding (~250 LOC of clone). One generic
   `json_dir_store<T>` would carry it all. Finding B-COR-2.

3. **`AdminRuntimeState` is a god-struct** mixing two listeners' worth of
   state (runtime + admin), keeping parallel maps in sync by hand, and
   carrying twelve fields that are dead when `admin_enabled = false`.
   Findings B-COR-7 and B-COR-9.

4. **Dialog scaffolding is hand-rolled per file across ~25 SPA dialogs**,
   with three different error-display patterns. A `useFormDialog` +
   `useConfirmMutation` pair would erase ~500 LOC and normalise the
   error UX. Findings W-DLG-1, W-DLG-2.

5. **`api/client.ts` is 75% wrapper boilerplate** — 500 lines of
   `unwrap(client.GET("/path", { params }))` one-liners. openapi-fetch
   already provides type safety; the wrappers add nothing. Finding W-API-1.

6. **`storeName` vs `storeId`** — the SPA threads `storeName` through every
   per-store page but the value is always the store's `id`. Misleading
   variable name now lives in 30+ files. Finding S-1.

7. **`pub` surface escapes the crate everywhere it doesn't need to** — both
   the backend (`lib.rs` re-exports, `pub mod auth`, `pub` on internal DTOs)
   and the SPA (test-only seams exported from `client.ts`). Findings B-LIB-1,
   B-ADM-3, W-API-3.

8. **WHAT-comments are everywhere** — the project's "delete on sight"
   comment rule is being relaxed in practice across both halves. Not
   wrong individually; collectively, a sweep is warranted. Findings
   B-DOC-1, B-DOC-2.

---

## Findings

Findings are prefixed by domain:

- **S-N** — Seam (crosses both backend and SPA)
- **B-ADM-N** — Backend admin_api/
- **B-RT-N** — Backend runtime_api/
- **B-COR-N** — Backend non-admin core (lib, server, storage, registries, http)
- **B-CFG-N** — Backend config
- **B-DOC-N** — Backend doc / comment hygiene
- **W-API-N** — Web api/ + hooks
- **W-RTR-N** — Web router/shell
- **W-DLG-N** — Web dialogs (cross-page)
- **W-PG-N** — Web pages

Each finding cites file:line. Locations are absolute relative to the repo
root. Quotes are abbreviated; the original three-pass output is consulted
for full code where needed.

---

### Seam (S-N) — admin API contract

#### S-1 — `storeName` parameter actually holds the store *id*

- **Category:** naming
- **Severity:** must-fix
- **Location(s):** `web/src/hooks/use-store-selector.ts:18-30`;
  `web/src/components/policy-viewer-dialog.tsx:21-23`;
  `web/src/pages/wrapper-permissions/page.tsx:39,46`;
  `web/src/pages/secrets/dialogs/*`;
  `web/src/pages/secrets/libsecret/*`;
  effectively every per-store page or dialog.
- **Observation:** `useStoreSelector(availableStores)` defaults to
  `availableStores[0].id`; the value is then threaded everywhere as
  `storeName` and finally passed to API helpers as the path `id`. The
  variable name actively lies; the operator never sees this string in the
  UI but readers of the code do.
- **Recommendation:** rename end-to-end to `storeId`. Touches ~30 files,
  none of it semantically. The wire shape doesn't move.
- **Behavior change?** no.

#### S-2 — `StoreLockResponse` defined inline in a handler, all other admin DTOs live in `shared-protocol::service`

- **Category:** boundaries / size, naming
- **Severity:** must-fix
- **Location(s):** `crates/backend-backendd/src/admin_api/stores/lifecycle.rs:583-586`
- **Observation:** every other `Admin*Response` type lives in
  `underwrap_protocol::service`. `StoreLockResponse` (the only one defined
  inline next to its handler) ends up in the generated `schema.ts` under
  a different naming pattern than its peers.
- **Recommendation:** move to `shared-protocol::service` as
  `AdminStoreLockResponse`. The SPA regenerates from OpenAPI; ts-fetch
  callers update.
- **Behavior change?** yes — wire-type name rename. Greenfield permits.

#### S-3 — Logout endpoint OpenAPI says 204, handler returns 200 with `{}`

- **Category:** doc / comment hygiene, errors
- **Severity:** must-fix
- **Location(s):** `crates/backend-backendd/src/admin_api/auth/mod.rs:63,152-164`
- **Observation:** the `#[utoipa::path]` annotation declares the response
  as `status = 204`, but the handler returns `Json<EmptyObject>` which
  produces 200 with `{}`. SPA-side `postLogout` uses `unwrap`'s
  `undefined` fallback so it works, but the contract and the wire disagree.
- **Recommendation:** return `StatusCode::NO_CONTENT` directly and delete
  the `EmptyObject` struct.
- **Behavior change?** yes (200 → 204). SPA already tolerates either.

#### S-4 — `groupSecretEntries` defensively filters out slot values the backend never emits

- **Category:** dead code / API hygiene, doc / comment hygiene
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/secrets/model.ts:9,44-58`
- **Observation:** `SECRET_SLOTS = ["env", "files"]` and a runtime filter
  guard against `libsecret` showing up. Comment: "filtered out
  defensively — the backend doesn't emit them through this endpoint
  after the flat-KV bridge was retired".
- **Recommendation:** narrow the OpenAPI schema for this list endpoint to
  `enum: ["env","files"]` so the generated TS type carries the constraint;
  drop the runtime filter and the comment.
- **Behavior change?** no (runtime behaviour identical).

#### S-5 — `cleanupHint` is a freeform string the SPA dumps into `<pre>`

- **Category:** complexity, boundaries / size
- **Severity:** nice-to-have
- **Location(s):** backend produces it in
  `crates/backend-backendd/src/admin_api/stores/crud.rs` (store-delete
  response); rendered in `web/src/pages/stores/dialogs/delete.tsx:36-43,67-69`
- **Observation:** the backend ships a multi-line prose string with the
  cleanup command; the SPA dumps it verbatim. UX quality of the message
  is now a backend concern the SPA cannot iterate on.
- **Recommendation:** return structured `{ data_dir, command }` from the
  backend; let the SPA render the prose. Greenfield permits.
- **Behavior change?** yes — wire shape change.

#### S-6 — Pre-flight duplicate-pair check in SPA reaches into `existingPairs` instead of trusting backend

- **Category:** complexity, errors
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/wrapper-permissions/dialogs/create-pair.tsx:100-109`
- **Observation:** the SPA scans `existingPairs` for `(wrapper, resource_id)`
  before posting and sets a custom error if it'd be a dup. Races with
  another operator and duplicates a constraint the backend already enforces.
  When this fires *after* a new resource was just created in the same
  dialog, the resource is left behind.
- **Recommendation:** drop the pre-flight; let the backend's 409 with a
  stable error code feed `describeApiError`. Tie this to S-9 below
  (transactional create-with-new-resource) to remove the orphan-resource
  failure mode entirely.
- **Behavior change?** no (better backend error mapping needed).

#### S-7 — Two query keys for "what permissions does this store have"

- **Category:** complexity, duplication
- **Severity:** nice-to-have
- **Location(s):** `web/src/components/policy-viewer-dialog.tsx:42-50` vs
  `web/src/pages/wrapper-permissions/page.tsx:44-48`
- **Observation:** the page fetches the aggregate
  `/v1/admin/stores/{id}/wrapper-permissions-view` (returns permissions +
  pairs + resources). The PolicyViewerDialog opens on the same page and
  fetches just `/v1/admin/stores/{id}/wrapper-permissions`. Two cache
  entries for the same store, two round-trips when the dialog opens.
- **Recommendation:** either pass `permissions` in as a prop from the page,
  or unify the two endpoints (have the page's existing view query feed
  both). The dialog already mounts inside the page.
- **Behavior change?** no.

#### S-8 — `LibsecretTree` fires one query per resource on page mount, even when its `<details>` is collapsed

- **Category:** complexity
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/secrets/libsecret/tree.tsx:31-34`
- **Observation:** for a store with N resources the secrets page makes N
  libsecret round-trips on mount, regardless of whether the operator ever
  expands any of them.
- **Recommendation:** gate with `enabled: open` driven by the `<details>`
  toggle, OR (cleaner) fold libsecret into the existing `*-view` endpoint
  so the page is one round-trip total. The backend already aggregates
  pairs/resources/permissions for the same store.
- **Behavior change?** yes (perf-positive).

#### S-9 — `CreatePairDialog` orchestrates two backend calls with no rollback

- **Category:** errors
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/wrapper-permissions/dialogs/create-pair.tsx:86-122`
- **Observation:** on "create new resource" path: create resource → check
  dups → create pair. If pair creation fails after the resource was
  created, the resource is orphaned silently.
- **Recommendation:** add a backend endpoint
  `POST /v1/admin/stores/{id}/pairs/with-new-resource` that does both in
  one transaction. Greenfield permits new endpoints. Pairs with S-6.
- **Behavior change?** yes — new endpoint, removes a failure mode.

#### S-10 — `cookies_secure` plumbed twice, hardcoded once

- **Category:** simplification, errors
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/server.rs:62,66`
- **Observation:** `csrf_config: build_csrf_config(true)` hardcodes
  secure-cookie; two lines later `cookies_secure: true` is stored on
  `AdminRuntimeState`. A future `cookies_secure: false` switch would
  leave CSRF cookies marked secure. Tests already use both `false` paths
  and only work because they pass `false` to both.
- **Recommendation:** derive `csrf_config` from `cookies_secure` in one
  place — pass `cookies_secure` into `build_csrf_config`.
- **Behavior change?** no (current values coincide).

---

### Backend admin_api (B-ADM-N)

#### B-ADM-1 — `admin_router_and_openapi` exists only for `.0` extraction

- **Category:** dead code / API hygiene
- **Severity:** must-fix
- **Location(s):** `crates/backend-backendd/src/admin_api/mod.rs:87-140,166-182`
- **Observation:** `admin_router_and_openapi` is only called by
  `admin_router` itself (`.0`). The "Backwards-compatible wrapper"
  comment is misleading — there is no consumer of the OpenApi half of the
  tuple. The drift test takes a separate path via `admin_openapi_spec()`.
  Two router-construction code paths must stay in lockstep, with no
  automated check that they do.
- **Recommendation:** keep `admin_router(state) -> Router` and
  `admin_openapi_spec() -> OpenApi` as the two public surfaces. Have
  `admin_openapi_spec` and `admin_router` share a common
  `build_openapi_router()` helper that returns
  `OpenApiRouter<Arc<AdminRuntimeState>>`; the live path applies tower
  layers + `.split_for_parts().0`, the spec path takes
  `.split_for_parts().1`. Single source of truth.
- **Behavior change?** no.

#### B-ADM-2 — `openapi::admin_openapi_spec` is a one-line forwarder

- **Category:** duplication, API hygiene
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/mod.rs:169-174`,
  `crates/backend-backendd/src/openapi.rs:19-21`
- **Observation:** `openapi::admin_openapi_spec` forwards verbatim to
  `admin_api::admin_openapi_spec`. Only consumer is the drift test.
- **Recommendation:** pick one. `pub use crate::admin_api::admin_openapi_spec`
  from `lib.rs` and delete the forwarder, or drop the `admin_api` version's
  pub-ness entirely.
- **Behavior change?** no.

#### B-ADM-3 — `pub` on internal admin DTOs widens visibility unnecessarily

- **Category:** dead code / API hygiene
- **Severity:** must-fix
- **Location(s):** `crates/backend-backendd/src/admin_api/auth/mod.rs:43,49,54,63`
  (`LoginRequest`, `LoginResponse`, `WhoamiResponse`, `EmptyObject`);
  `crates/backend-backendd/src/admin_api/auth/bootstrap.rs:29,35,40`;
  `crates/backend-backendd/src/admin_api/stores/lifecycle.rs:583`
- **Observation:** `pub mod auth` and `pub struct LoginRequest` claim a
  wider audience than they have. The crate's own `lib.rs:23-26` is the
  only place that re-exports them; inside the crate everything is
  `pub(crate)`-shaped already.
- **Recommendation:** demote to `pub(crate)` (or `pub(super)`). `lib.rs`
  `pub use` of a `pub(crate)` item still works as long as the re-export
  doesn't escape the crate.
- **Behavior change?** no (pairs with B-LIB-1 which also drops the
  re-exports themselves).

#### B-ADM-4 — Identical password-validation-to-`AdminError` block in 4 sites

- **Category:** duplication
- **Severity:** must-fix
- **Location(s):** `crates/backend-backendd/src/admin_api/users.rs:205-214,296-305`;
  `crates/backend-backendd/src/admin_api/auth/mod.rs:211-220`;
  `crates/backend-backendd/src/admin_api/auth/bootstrap.rs:145-155`
- **Observation:** each site does the same `if let Err(code) =
  validate_password(...)` + `match code → message string`. The bootstrap
  version uses slightly different messages — purely cosmetic.
- **Recommendation:** extract
  `fn validate_password_or_bad_request(password: &str) -> Result<(), AdminError>`
  used by all four. The code field is what the SPA switches on; the
  messages only need to be consistent.
- **Behavior change?** wire-message strings change for two endpoints;
  codes unchanged.

#### B-ADM-5 — Triplicated dependent-permission scan

- **Category:** duplication
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/pairs.rs:165-179`,
  `crates/backend-backendd/src/admin_api/resources.rs:182-196`
- **Observation:** both delete handlers do `policy.permissions.iter().filter(predicate).take(SAMPLE_LIMIT)`
  with `u32::try_from(...).unwrap_or(u32::MAX)`-style counters.
- **Recommendation:** helper next to `resource_secret_dependents` in
  `storage::documents` taking a predicate, returning `(count_u32,
  Vec<AdminWrapperPermissionKey>)`.
- **Behavior change?** no.

#### B-ADM-6 — `saturating_u32` used in one place, open-coded everywhere else

- **Category:** duplication, inconsistency
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/stores/mod.rs:59-61`
  (defines it); `pairs.rs:177`, `resources.rs:178,194`, `storage/documents.rs:95`
  (open-code the same `u32::try_from(...).unwrap_or(u32::MAX)`).
- **Recommendation:** promote `saturating_u32` to a single shared helper.
- **Behavior change?** no.

#### B-ADM-7 — `audit_result` lives in `libsecret.rs` but its purpose is generic

- **Category:** boundaries / size, duplication
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/libsecret.rs:140-142`;
  open-coded in `secrets.rs:182-186,232-236`, `resources.rs:295-296`,
  `users.rs:128-156`.
- **Recommendation:** move `audit_result` (or rename to `result_label`)
  to `admin_api::audit` so every handler emitting both success and
  failure branches uses it.
- **Behavior change?** no.

#### B-ADM-8 — `mutate_collection` over-engineered diff-and-save for one-record mutations

- **Category:** complexity, simplification
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/users.rs:48-74`
- **Observation:** each handler mutates exactly one record. The helper
  snapshots the full `Vec<UserRecord>` before and after, then runs an
  O(n) diff with two `HashMap<&str, &UserRecord>` allocations to figure
  out which records changed.
- **Recommendation:** either return a small `Changed { saved, deleted }`
  from the closure, or move persistence into the closure's caller (each
  handler knows its id).
- **Behavior change?** no.

#### B-ADM-9 — `mutate_collection` returns `anyhow::Result`, forcing `downcast_ref` for typed errors

- **Category:** errors, awkward abstraction
- **Severity:** must-fix
- **Location(s):** `crates/backend-backendd/src/admin_api/users.rs:48-74,107-115`
- **Observation:** the closure returns `anyhow::Result<T>`; handlers then
  `downcast_ref::<UserMutationError>()` and fall back to "treat as a 400
  invalid_user". An IO error inside `save_user` would currently surface
  as a 400 with the IO message in the body.
- **Recommendation:** parameterise the closure as
  `FnOnce(&mut UserCollection) -> Result<T, UserMutationError>` and have
  `mutate_collection` return
  `Result<T, MutateCollectionError>` where `MutateCollectionError ∋
  User(UserMutationError) | Io(anyhow::Error)`. Handlers `match` cleanly;
  IO maps to 500.
- **Behavior change?** yes (correctness fix — IO errors stop being 400s).

#### B-ADM-10 — `audit_user_mutation`'s `success_username` extractor takes a `FnOnce(&T) -> &str`

- **Category:** complexity, simplification
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/users.rs:123-157`;
  callers at `:239,273,325,353`.
- **Observation:** four handlers each pass a tiny closure (`|r| &r.username`,
  `|s| s.as_str()`, …) to handle inconsistent success types.
- **Recommendation:** normalise to `String` at the call site so
  `audit_user_mutation` takes just `(action, principal, username, result)`.
  Removes the generic parameter.
- **Behavior change?** no.

#### B-ADM-11 — `AdminError::new` is leaked to `pub(crate)` for a one-file convenience

- **Category:** dead code / API hygiene
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/error.rs:37-47`
- **Observation:** no caller outside the file constructs via `AdminError::new`.
- **Recommendation:** make `new` a private inherent method.
- **Behavior change?** no.

#### B-ADM-12 — `AdminError::store_not_found` exists for one string; peers don't

- **Category:** simplification
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/error.rs:57-59`;
  usages in `stores/crud.rs`, `stores/lifecycle.rs`, `stores/mod.rs`.
- **Observation:** `store_not_found` is the only such alias;
  `not_found("unknown client instance")` / `not_found("unknown resource")`
  are open-coded.
- **Recommendation:** either add peer aliases for symmetry
  (`client_instance_not_found`, `resource_not_found`), or inline
  `store_not_found` and drop the helper. Pick one.
- **Behavior change?** no.

#### B-ADM-13 — Live-store-lookup boilerplate duplicated in 4 store handlers

- **Category:** duplication
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/stores/crud.rs:301-313,354-371`;
  `stores/lifecycle.rs:84-94,424-438`.
- **Observation:** four sites repeat `let reg = state.stores.read().unwrap(); let
  loaded = reg.records.get(&name).ok_or_else(AdminError::store_not_found)?;
  match reg.state.get(&name) { Some(Unlocked(b)) => …, Some(Locked) => …,
  None => return Err(internal_with_code("internal", "store metadata
  present but registry state missing")) }`.
- **Recommendation:** add a `StoreRegistry::admin_store_state(id) ->
  Result<(StoreRecord, StoreState), RegistryLookupError>` and let
  handlers be a single match arm. Pairs with B-COR-3 (which collapses
  the two-map registry to one).
- **Behavior change?** no.

#### B-ADM-14 — `reencrypt_into` repeats cleanup+log+return boilerplate 5 times

- **Category:** duplication, complexity
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/stores/lifecycle.rs:297-380`
- **Observation:** each error branch does
  `let _ = remove_dir_all(stage_dir);` + `log_admin_event(...)` + `return
  Err(AdminError::internal_with_code(...))`. The function is 84 lines
  that compress to under 40 with a `bail!` closure capturing
  `stage_dir`, `principal`, `store_name`.
- **Recommendation:** extract the closure; same pattern applies to the
  enclosing `handle_admin_store_rotate_key`.
- **Behavior change?** no.

#### B-ADM-15 — `parse_slot` arms re-construct the parsed value

- **Category:** simplification
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/secrets.rs:55-70`
- **Observation:** `Some(ResourceSlot::Env) => Ok(ResourceSlot::Env)` and
  the same for `Files`.
- **Recommendation:** `Some(s @ (ResourceSlot::Env | ResourceSlot::Files))
  => Ok(s)`.
- **Behavior change?** no.

#### B-ADM-16 — `force` query parsing accepts empty string as truthy

- **Category:** simplification, errors
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/stores/crud.rs:349-352`,
  `:322-326`
- **Observation:** `query.force.as_deref().is_some_and(|v| matches!(v,
  "true" | "1" | ""))`. `?force=` is treated identically to `?force=true`.
  Also: `force: Option<String>` should just be `bool`.
- **Recommendation:** drop the empty-string match; better, parse to
  `bool` via serde with a custom deserializer (or just `serde(default)`
  on a `bool` field — TS-side can stop sending the literal "true"/"1").
- **Behavior change?** yes (very small) — `?force=` (literal empty
  value) stops bypassing the empty-store guard.

#### B-ADM-17 — Redundant `#[serde(default)]` on `Option<T>` fields

- **Category:** simplification, doc / comment hygiene
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/secrets.rs:72-78`,
  `crates/backend-backendd/src/admin_api/stores/crud.rs:322-326`
- **Observation:** `Option<T>` already deserializes from missing fields
  as `None`; `#[serde(default)]` is redundant.
- **Recommendation:** delete the `#[serde(default)]` on `Option<T>` fields.
- **Behavior change?** no.

#### B-ADM-18 — `rotation_locks` map grows unbounded; deleting a store leaves stale entries

- **Category:** dead code / API hygiene
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/stores/lifecycle.rs:35-42`;
  map in `runtime_state.rs` as
  `rotation_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>`.
- **Observation:** entries inserted on first rotation, never removed.
  Not a security leak, but the per-store lock leaks alongside the
  registry-vs-state inconsistency theme.
- **Recommendation:** drop the entry in `handle_admin_store_delete`, OR
  (cleaner — folds into B-COR-3) embed the `Mutex<()>` on `StoreState`
  so it lives and dies with the registry record.
- **Behavior change?** no.

#### B-ADM-19 — `bootstrap_completed(...).unwrap_or(true)` silently swallows IO errors

- **Category:** errors, logging
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/auth/bootstrap.rs:66,67-68`
- **Observation:** an IO failure mapping to "bootstrap not required"
  hides operator-visible misconfigurations. Same with
  `crate::users::load(...).unwrap_or(false)` on the next line.
- **Recommendation:** `tracing::warn!(err = ?e, "…")` before the fallback.
- **Behavior change?** logging only.

#### B-ADM-20 — `policy::drop_client_instance_permissions` silently swallows "store locked or missing"

- **Category:** errors
- **Severity:** must-fix
- **Location(s):** `crates/backend-backendd/src/admin_api/policy.rs:34-53`
- **Observation:** `Err(_) => return Ok(())` for both `Locked` and
  `NotFound`. Callers in `client_instances.rs:343-357,410-421` warn only
  if the cascade fails after the lookup succeeded. When the store is
  *locked*, orphaned permissions stay on disk silently — no warning, no
  audit line. Contradicts the docstring claim that the function exists
  "so authorizations don't silently survive across the reassignment".
- **Recommendation:** match on `RegistryLookupError` explicitly. For
  `Locked`, emit `tracing::warn!` and an audit line, and consider
  refusing the parent operation. For `NotFound`, `tracing::info!` and
  return `Ok(())` is fine.
- **Behavior change?** yes — gives operators visibility into a current
  silent gap.

#### B-ADM-21 — `handle_admin_login`'s `axum_login::Error` over-match doesn't pay for itself

- **Category:** simplification
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/auth/mod.rs:99-129`
- **Observation:** three different 500s with slightly different messages,
  all with `code: "internal"`. Operators get no signal from the message
  variance; the structured error already has more context.
- **Recommendation:** wildcard arm with `tracing::warn!` + single
  `AdminError::internal_with_code("internal", "login failed")`.
- **Behavior change?** wire-message text simplifies.

#### B-ADM-22 — `resolve_principal_layer` re-reads the full user collection on every protected request

- **Category:** complexity, simplification
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/csrf.rs:154-204`
- **Observation:** the layer calls `users::load(&state.users_dir)` per
  request so a rename "takes effect immediately for this admin". The
  session already carries a `UserRecord`; the freshness window after a
  rename by another admin is at most one request (axum_login's
  session-hash check handles that case).
- **Recommendation:** build `Principal` directly from `auth_session.user`.
  If immediate-rename matters for the renaming admin, do that refresh
  inside the rename handler.
- **Behavior change?** yes (small) — rename triggered by another admin
  becomes visible on the next session-hash refresh rather than immediately.

#### B-ADM-23 — `decode_b64` is STANDARD-only; URL-safe input 400s

- **Category:** errors, API hygiene
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/body_b64.rs:16-22`
- **Observation:** brittle to a SPA-side encoder switch. Not a current
  bug — the SPA emits STANDARD — but base64url-default TS encoders are
  common.
- **Recommendation:** decode permissively (STANDARD + URL_SAFE_NO_PAD
  fallback). Document STANDARD-out, permissive-in in the module rustdoc.
- **Behavior change?** no.

#### B-ADM-24 — `body_b64` module name doesn't describe what it does

- **Category:** naming
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/body_b64.rs`
- **Observation:** module has nothing to do with HTTP bodies; two
  general-purpose base64 helpers.
- **Recommendation:** fold into a `util.rs` next to `saturating_u32`,
  `result_label`, and `IntoInternal` (see B-ADM-25).
- **Behavior change?** no.

#### B-ADM-25 — `AdminError` boilerplate around storage operations dominates handlers

- **Category:** duplication
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/error.rs:10-13`;
  dozens of sites (e.g. `pairs.rs:48-50,92-93,99-100,120-121`,
  `resources.rs`, `secrets.rs`).
- **Observation:** the module-level comment says `From<anyhow::Error>` is
  "deliberately not implemented" so handlers pick the context string
  explicitly. The result is `map_err(|e| AdminError::internal("loading X
  document", e))?` repeated everywhere.
- **Recommendation:** add a `pub(crate) trait IntoInternal { fn
  into_internal(self, ctx: &'static str) -> AdminError; }` impl for
  `anyhow::Error`. Call sites read `load_pairs_document(...).map_err(|e|
  e.into_internal("loading pairs document"))?`. Keeps the explicit-
  context invariant.
- **Behavior change?** no.

#### B-ADM-26 — `info!("admin request", method, path, …)` logs include user-controlled URL segments

- **Category:** logging
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/csrf.rs:169-174`
- **Observation:** `request.uri().path()` includes resource ids, libsecret
  item ids, alias names — operator-chosen strings that may be
  customer-recognisable. Per-handler `log_admin_event` already carries
  the structured detail with redaction discipline.
- **Recommendation:** log `method` + first-N path segments only, or drop
  the request-level info entirely now that the structured per-handler
  events exist.
- **Behavior change?** no — log shape change only.

#### B-ADM-27 — Inline `use` statements stashed inside function bodies

- **Category:** doc / comment hygiene, naming
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/admin_api/pairs.rs:80,156`;
  `resources.rs:90,153,263`; `auth/mod.rs:208-209`.
- **Observation:** `use underwrap_protocol::entities::{...};` stashed
  inside handler bodies. No imports collide. Module-level imports become
  incomplete.
- **Recommendation:** hoist into the file's top-level `use` block.
- **Behavior change?** no.

---

### Backend runtime_api (B-RT-N)

#### B-RT-1 — Two response shapes inside one HTTP listener

- **Category:** duplication, simplification, complexity
- **Severity:** must-fix
- **Location(s):** `crates/backend-backendd/src/http/responses.rs` vs
  `crates/backend-backendd/src/runtime_api/responses.rs`; every handler
  in `runtime_api/`.
- **Observation:** admin returns `axum::response::Response` via
  `AdminError::into_response()`. Runtime returns `hyper::Response<Full<Bytes>>`
  via hand-built builders (`json_response`, `error_response`,
  `store_locked_response`). Two response dialects in one process. Forces
  every runtime handler to spell `match load_…(store) { Ok(d) => d, Err(e)
  => return error_response(…, &format!("loading X: {e:#}")) }` instead
  of `?`-ing through an `IntoResponse` impl.
- **Recommendation:** collapse onto `axum::response::Response`. Build a
  `RuntimeError` type with `IntoResponse` mirroring the admin envelope.
  Handlers return `Result<impl IntoResponse, RuntimeError>` and use `?`.
  Removes ~30 lines per runtime handler (≈ 150 LOC total).
- **Behavior change?** no — wire shape identical; Rust-API change only.

#### B-RT-2 — Repeated `match load_X_document` boilerplate

- **Category:** duplication
- **Severity:** must-fix (pairs with B-RT-1)
- **Location(s):** `crates/backend-backendd/src/runtime_api/wrappers.rs:68-87,154-172,228-264`;
  `runtime_api/invocations.rs:101-119,192-211,312-353`.
- **Observation:** each handler repeats the 5-7-line match-and-format
  block. `wrappers.rs` alone has six near-identical blocks.
- **Recommendation:** subsumed by B-RT-1's `RuntimeError` consolidation
  plus an `IntoInternal`-style adapter for the runtime side.
- **Behavior change?** no.

#### B-RT-3 — `handle_runtime_wrapper_register` is 240 lines, three flows interleaved

- **Category:** boundaries / size, complexity
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/runtime_api/wrappers.rs:32-273`
- **Observation:** the function mixes input validation, loading three
  documents, computing missing-resources/missing-pairs, the no-op fast
  path with permission seeding, auto-create-disabled refusal, and the
  commit path with sequential saves. Six exit points return the same
  envelope.
- **Recommendation:** decompose into pure helpers:
  `compute_register_diff(body, &resources, &pairs) -> RegisterDiff`,
  `apply_auto_create(&mut resources, &mut pairs, &diff, creator)`,
  `seed_locked_permissions(&mut policy, body, resolved, principal)
  -> bool`, plus a driver that dispatches the three outcomes. End state
  is ~80 lines of orchestration + small unit-testable pieces.
- **Behavior change?** no.

#### B-RT-4 — `seed_locked_permissions` is a 27-line closure inside a 240-line function

- **Category:** complexity
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/runtime_api/wrappers.rs:122-148`
- **Observation:** mutates outer `PolicyDocument`, invoked at two
  branches. Used as a poor man's free function.
- **Recommendation:** lift to `fn`. Subsumed by B-RT-3.
- **Behavior change?** no.

#### B-RT-5 — `generate_resource_id` is a one-line wrapper with a stale comment

- **Category:** simplification, doc / comment hygiene
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/runtime_api/wrappers.rs:275-280`
- **Observation:** `pub(crate) fn generate_resource_id() -> String {
  crate::ids::random_id("res") }`. One caller. Docstring claims it
  "mirrors `generate_client_instance_id` in shape" — that function
  doesn't exist (the analogous path uses `fresh_id("inst", …)` inline).
- **Recommendation:** inline at the call site.
- **Behavior change?** no.

#### B-RT-6 — `validate_runtime_client_mappings` and the admin delete handler enforce the same invariant in two places

- **Category:** complexity
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/runtime_api/principal.rs:22-43`;
  the constraint must also be maintained by
  `crates/backend-backendd/src/admin_api/stores/crud.rs:421` (store-delete).
- **Observation:** invariant lives in two places that can drift; the
  data model permits the inconsistency on disk.
- **Recommendation:** out of scope for a pure-refactor pass; file a
  follow-up to think about FK-style modeling. Short term: add a
  comment in `validate_runtime_client_mappings` pointing at the
  admin handler that maintains the same invariant.
- **Behavior change?** no.

#### B-RT-7 — Existence check duplicated in `resolve_runtime_client` + `unlocked_backend`

- **Category:** duplication
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/runtime_api/principal.rs:65-73`;
  `runtime_api/mod.rs:76-95`.
- **Observation:** two lock acquisitions, two existence checks for one
  fact. Pairs with B-COR-3 (which makes the registry the single source
  of truth, eliminating the possibility of a stale id).
- **Recommendation:** drop the existence check in `resolve_runtime_client`;
  let `unlocked_backend`'s `NotFound` arm carry the "unknown store" log.
- **Behavior change?** no.

#### B-RT-8 — Duplicated structured-field-and-message logging

- **Category:** logging
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/runtime_api/mod.rs:65-68`
- **Observation:** `warn!(err = format!("{e:#}"), "rejecting … {e:#}")` —
  the JSON output carries the same string twice.
- **Recommendation:** keep the structured `err` field, drop the
  interpolation from the message.
- **Behavior change?** no.

---

### Backend non-admin core (B-COR-N)

#### B-COR-1 — `lib.rs` re-exports a wall of types nobody outside the crate consumes

- **Category:** dead code / API hygiene
- **Severity:** must-fix
- **Location(s):** `crates/backend-backendd/src/lib.rs:23-33`
- **Observation:** workspace-wide grep finds zero consumers of
  `BootstrapRequest`, `BootstrapResponse`, `BootstrapStatusResponse`,
  `LoginRequest`, `LoginResponse`, `WhoamiResponse`, `DEFAULT_STORES_DIR`,
  `LoadedStoreRecord`, `RegistryLookupError`, `StoreRegistry`, `StoreState`,
  `is_valid_store_id`, `load_store_records`. External callers
  (integration-tests, the binary, `tests/openapi_export.rs`) use
  `BackendConfig`, `HttpsConfig`, `HttpsTlsConfig`, `AdminConfig`,
  `cli::*`, `server::run`, `openapi::admin_openapi_spec`,
  `store_registry::save_store_record`, `runtime_clients::*` only.
- **Recommendation:** delete the `pub use` block; demote `pub mod auth`,
  `pub mod web`, `pub mod tls`, `pub mod users` to `pub(crate) mod …`.
  Leave `pub mod cli`, `pub mod config`, `pub mod openapi`,
  `pub mod server`, `pub mod store_registry`, `pub mod runtime_clients`.
- **Behavior change?** no.

#### B-COR-2 — Three near-clone on-disk-registry modules

- **Category:** duplication
- **Severity:** must-fix
- **Location(s):** `crates/backend-backendd/src/users.rs:232-364`;
  `crates/backend-backendd/src/store_registry.rs:179-313`;
  `crates/backend-backendd/src/runtime_clients.rs:101-280`
- **Observation:** three modules each implement `record_path`,
  `load_one`, `load(dir)` with the same read_dir+filter+sort+uniqueness
  scan, `save_*` with tempfile+sync+persist, NotFound-tolerant `delete_*`.
  Same `with_context` formatting and the same anyhow error wording. The
  only per-entity logic is: validate id, validate record fields, dedupe
  key.
- **Recommendation:** lift to a private `json_dir_store<T: DirRecord>`
  module. The trait carries `fn id(&self) -> &str`,
  `fn validate(&self, path: &Path) -> Result<()>`,
  `fn uniqueness_keys(&self) -> impl Iterator<Item=(&'static str,
  String)>`, plus `fn set_id(&mut self, id: String)` to handle the
  `#[serde(default, skip)] pub id: String` round-trip. The three modules
  shrink to ~30 lines each.
- **Behavior change?** no.

#### B-COR-3 — `LoadedStoreRecord` is a zero-value newtype, and `state`/`records` are parallel maps

- **Category:** simplification
- **Severity:** must-fix
- **Location(s):** `crates/backend-backendd/src/store_registry.rs:26-30,70-73`
- **Observation:** `LoadedStoreRecord { record: StoreRecord }` is a
  one-field wrapper with no methods. Callers all write `.record.foo`. In
  addition, `StoreRegistry { state, records }` keeps two parallel maps
  by the same id; every CRUD path in `admin_api/stores/crud.rs` must
  remember to update both.
- **Recommendation:** delete `LoadedStoreRecord`; collapse to a single
  `HashMap<String, StoreEntry>` where
  `StoreEntry { record: StoreRecord, state: StoreState }`. Pairs with
  B-ADM-18 (embed `rotation_lock: Mutex<()>` on `StoreEntry`).
- **Behavior change?** no.

#### B-COR-4 — Three `is_valid_*_id` wrappers all alias `ids::is_valid_simple_id`

- **Category:** simplification
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/users.rs:420-422`;
  `store_registry.rs:318-320`; `runtime_clients.rs:282-284`.
- **Recommendation:** drop the three wrappers, call
  `ids::is_valid_simple_id` directly. Rename it to `ids::is_valid_id`
  since the `simple` qualifier is unused.
- **Behavior change?** no.

#### B-COR-5 — `StoreState::is_locked()`, `StoreRegistry::new()`, `cert_fingerprint_from_pem_file` are dead

- **Category:** dead code / API hygiene
- **Severity:** must-fix
- **Location(s):** `crates/backend-backendd/src/store_registry.rs:42-53,75-81`;
  `crates/backend-backendd/src/runtime_clients.rs:95-99`.
- **Observation:** `StoreState::is_locked()` (inherent) has no
  non-test callers. `StoreRegistry::new()` is test-only.
  `cert_fingerprint_from_pem_file` has zero callers.
- **Recommendation:** delete. Tests can use struct literals.
- **Behavior change?** no.

#### B-COR-6 — `StoreRegistry::unlock` advertises a return value nobody uses

- **Category:** dead code / API hygiene
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/store_registry.rs:104-115`;
  caller `crates/backend-backendd/src/admin_api/stores/lifecycle.rs:514-516`.
- **Observation:** doc says "returns previous state so the caller can
  error on `Unlocked` to avoid a race". Caller ignores the return value.
- **Recommendation:** change return type to `()` and drop the doc
  paragraph.
- **Behavior change?** no.

#### B-COR-7 — `AdminRuntimeState` is a god-struct mixing two listeners' state

- **Category:** boundaries / size
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/runtime_state.rs:22-93`
- **Observation:** struct fuses shared registries with twelve admin-only
  fields. When `admin_enabled = false`, twelve of the fields are unread.
- **Recommendation:** split into `SharedState { stores, client_instances
  }` and `AdminState { session_store, auth_backend, csrf_config,
  session_lifetime_secs, cookies_secure, bootstrap_lock,
  user_collection_lock, allowed_origins, users_dir, bootstrap_file,
  stores_data_root, rotation_locks }`. Carry as `Arc<SharedState>` +
  `Option<Arc<AdminState>>`. Rename the type from `AdminRuntimeState`
  to something honest.
- **Behavior change?** no.

#### B-COR-8 — `Principal` lives in `users.rs` despite being a request-context concept

- **Category:** naming
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/users.rs:89-93`
- **Observation:** `users.rs` is the on-disk-records module per its own
  rustdoc. `Principal` is consumed by `admin_api/*` only.
- **Recommendation:** move to `admin_api::csrf` (where it's resolved).
- **Behavior change?** no.

#### B-COR-9 — Manual `Debug for UserCollection` impl is unnecessary

- **Category:** dead code / API hygiene, doc / comment hygiene
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/users.rs:101-110`
- **Observation:** the comment says we're keeping a manual impl to
  preserve `UserRecord`'s redaction. But `#[derive(Debug)]` on
  `UserCollection` would use `UserRecord::Debug` automatically.
- **Recommendation:** delete the manual impl, derive `Debug`, drop the
  comment.
- **Behavior change?** no.

#### B-COR-10 — `recover_user` re-uses `UserCollection::add` for invariants that are vacuously true

- **Category:** simplification
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/cli.rs:21-52`
- **Observation:** comment claims "Re-use `UserCollection::add` for its
  uniqueness + hash invariants; we throw the collection away". But
  `collection.find_by_username` already checked dup on line 30 and bailed
  early; the hash was just produced by `auth::hash_password` so the
  PHC-prefix check is vacuous.
- **Recommendation:** delete lines 47-49 and the misleading comment;
  save the record directly.
- **Behavior change?** no.

#### B-COR-11 — Duplicate `load_*` / `save_*` document helpers in storage

- **Category:** duplication
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/storage/documents.rs:25-85`
- **Observation:** six functions of identical shape (`load_policy_document`,
  `load_resources_document`, `load_pairs_document`, plus three `save_*`).
- **Recommendation:** collapse to two generics: `fn load_document<T:
  Default + DeserializeOwned>(store, id) -> Result<T>` and `fn
  save_document<T: Serialize>(store, id, doc) -> Result<()>`. Keep
  thin per-document aliases if call sites read better that way.
- **Behavior change?** no.

#### B-COR-12 — `load_bootstrap_sentinel` is only exercised by its own unit test

- **Category:** dead code / API hygiene
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/users.rs:376-387`
- **Observation:** production reads only `bootstrap_completed(&path) ->
  bool`. The sentinel's `completed_at` / `by_username` fields are
  write-only on disk.
- **Recommendation:** either surface the structured record via
  `GET /v1/admin/bootstrap-status` (operator-useful), or delete the
  struct and write a presence-only sentinel file.
- **Behavior change?** depends on direction.

#### B-COR-13 — `now_epoch_secs` and `new_user_id` are general-purpose helpers in a domain module

- **Category:** naming
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/users.rs:216-229`
- **Observation:** `now_epoch_secs` is imported via `users::` from
  `runtime_api/wrappers.rs`, `runtime_api/invocations.rs`,
  `admin_api/resources.rs`, `admin_api/pairs.rs` etc. The
  `users::` prefix is misleading at runtime call sites.
- **Recommendation:** move `now_epoch_secs` to `crate::time` (or to
  `ids.rs`). `new_user_id` can stay if it's a user-specific factory; the
  underlying random-hex generator could live next to `ids::random_id`.
- **Behavior change?** no.

#### B-COR-14 — `Body<T>` extractor has `pub` field on a `pub(crate)` type

- **Category:** dead code / API hygiene
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/http/responses.rs:27`
- **Observation:** `pub(crate) struct Body<T>(pub T);` — inconsistent.
- **Recommendation:** field → `pub(crate)`.
- **Behavior change?** no.

#### B-COR-15 — `is_locked(store_id)` covers the same fact as `unlocked_backend`'s `Locked` arm

- **Category:** simplification
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/store_registry.rs:100-103`
- **Observation:** `is_locked` returns `bool`, swallows NotFound (returns
  `false` for unknown ids). Only test-callers.
- **Recommendation:** mark `#[cfg(test)]` or delete; tests use
  `matches!(unlocked_backend(id), Err(RegistryLookupError::Locked))`.
- **Behavior change?** no.

#### B-COR-16 — `spa_unavailable()` HTML lives in production code for a build misconfiguration

- **Category:** greenfield cleanup
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/web.rs:92-107`
- **Observation:** returns a 503 with developer instructions. Reachable
  only when a dev runs `cargo build` without `npm run build`; the Nix
  closure guarantees the bundle for non-dev environments.
- **Recommendation:** replace with a compile-time check (`build.rs`
  assertion or `compile_error!` macro reading an `OPTION_ENV`). A
  release-quality binary should not carry this HTML fallback.
- **Behavior change?** yes — `cargo build` without `npm run build`
  refuses to build. Safe because that's already a hard requirement.

#### B-COR-17 — `tls.rs` carries a `supported_algs` instance field redundant with `ring::default_provider()`

- **Category:** doc / comment hygiene, simplification
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/tls.rs:36-58`
- **Observation:** `supported_algs` is an instance field even though it's
  just `provider.signature_verification_algorithms`. Two providers are
  summoned (lines 20, 53); hidden coupling.
- **Recommendation:** drop the instance field; reach for
  `default_provider().signature_verification_algorithms` directly in
  the verifier methods. Or document that `install_default` and
  `default_provider` resolve to the same instance.
- **Behavior change?** no.

---

### Backend config (B-CFG-N)

#### B-CFG-1 — `Option<Vec<String>>` allowed_origins where `Vec<String>` would do

- **Category:** simplification
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/config.rs:28-29`;
  `crates/backend-backendd/src/http/origin.rs:46-49`.
- **Observation:** `check_admin_origin` collapses `None` and
  `Some(vec![])` to the same "no allowlist" behavior.
- **Recommendation:** `allowed_origins: Vec<String>` with
  `#[serde(default)]`. Empty vec is the unset signal.
- **Behavior change?** no.

#### B-CFG-2 — `Option<u64>` session_lifetime_secs always resolved to default

- **Category:** simplification
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/config.rs:32-33`;
  `crates/backend-backendd/src/server.rs:63-65`.
- **Recommendation:** `session_lifetime_secs: u64` with `#[serde(default
  = "default_session_lifetime_secs")]`.
- **Behavior change?** no.

#### B-CFG-3 — Four `Option<PathBuf>` config dirs that always have a default-resolver

- **Category:** simplification
- **Severity:** nice-to-have
- **Location(s):** `crates/backend-backendd/src/config.rs:51-71,78-105`
- **Observation:** four optional dirs with four `*_dir()` resolver
  methods that `.unwrap_or_else(|| PathBuf::from(DEFAULT_…))`. Same trick
  four times.
- **Recommendation:** non-optional fields with `#[serde(default =
  "default_*")]`. The four resolver methods disappear.
- **Behavior change?** no.

---

### Backend doc / comment hygiene (B-DOC-N)

#### B-DOC-1 — WHAT-comments narrating the next line

- **Category:** doc / comment hygiene
- **Severity:** nice-to-have
- **Location(s):**
  - `crates/backend-backendd/src/admin_api/mod.rs:90-110` (section banners
    "tower-sessions + axum-login", "axum_csrf", "OpenApiRouter composition")
  - `crates/backend-backendd/src/admin_api/csrf.rs:138-140` (narrating
    what `IntoResponse` on a tuple does)
  - `crates/backend-backendd/src/admin_api/secrets.rs:332-334`
  - `crates/backend-backendd/src/admin_api/auth/bootstrap.rs:170-171`
  - `crates/backend-backendd/src/admin_api/libsecret.rs:58,144,197,330,499`
    (`---- shared helpers ----` banners)
- **Recommendation:** sweep these. Keep WHY-comments
  (atomicity invariants, security-critical decisions); cut WHATs.
- **Behavior change?** no.

#### B-DOC-2 — Stale module-level rustdoc

- **Category:** doc / comment hygiene
- **Severity:** nice-to-have
- **Location(s):**
  - `crates/backend-backendd/src/admin_api/audit.rs:10-12` — "Per-subdomain
    wrappers used to format the entity string by hand; they have all been
    replaced by the `AuditEntity` enum…" — archaeology
  - `crates/backend-backendd/src/admin_api/mod.rs:166-168` — admin_openapi_spec
    doc references the dual-construction antipattern
  - `web/src/api/client.ts:516` — comment "of these go through the
    legacy flat-KV bridge" referring to a deleted code path
- **Recommendation:** delete the archaeology paragraphs; keep
  forward-facing descriptions only.
- **Behavior change?** no.

---

### Web api/ + hooks (W-API-N)

#### W-API-1 — `api/client.ts` is 75% wrapper boilerplate

- **Category:** boundaries / size, simplification
- **Severity:** must-fix
- **Location(s):** `web/src/api/client.ts:156-669`
- **Observation:** ~500 lines of one-line-per-endpoint helpers of the
  form `return unwrap(client.METHOD("/path/{x}", { params: { path: { x
  } }, body }));`. openapi-fetch is already type-safe.
- **Recommendation:** drop the wrappers. Either inline `client.GET(...)`
  at call sites, or expose `api.GET / api.POST / api.PUT / api.DELETE`
  that wrap `unwrap` + the client once. Keep the named-type
  re-exports the call sites actually import.
- **Behavior change?** no.

#### W-API-2 — Local-only type aliases bulk the `api/client.ts` header

- **Category:** dead code / API hygiene
- **Severity:** nice-to-have
- **Location(s):** `web/src/api/client.ts:109-154`
- **Observation:** ~12 `type` aliases (e.g. `AdminClientInstanceUpdateRequest`,
  `AdminPairCreateRequest`) exist only because a single wrapper consumes
  them.
- **Recommendation:** subsumed by W-API-1 — when the wrappers go, the
  local-only aliases go.
- **Behavior change?** no.

#### W-API-3 — `_resetCsrfTokenForTesting` is a production export for tests

- **Category:** dead code / API hygiene
- **Severity:** nice-to-have
- **Location(s):** `web/src/api/client.ts:48-51`
- **Observation:** the only `_*ForTesting` export in the SPA. Out of
  character with the rest of the codebase.
- **Recommendation:** replace with `vi.resetModules()` in tests, or
  guard with `import.meta.env.MODE`.
- **Behavior change?** no.

#### W-API-4 — `unwrap`'s `(undefined as unknown) as T` cast hides 204 endpoints

- **Category:** complexity
- **Severity:** nice-to-have
- **Location(s):** `web/src/api/client.ts:89-102`
- **Observation:** the double-cast lets `unwrap<T>()` lie about returning
  `T` when in fact some endpoints return nothing.
- **Recommendation:** change signature to `Promise<T | undefined>` and
  let callers narrow. Or have void-returning endpoints declare it.
  Removes the cast.
- **Behavior change?** no.

#### W-API-5 — `useStoreSelector` writes URL state but never clears it

- **Category:** complexity
- **Severity:** nice-to-have
- **Location(s):** `web/src/hooks/use-store-selector.ts:18-30`
- **Observation:** on mount + storeName-change effect, always writes the
  URL; no path clears `?store=`. URL and state can drift.
- **Recommendation:** drop the URL-write effect; have the `<Select
  onChange>` update both state and URL (single source of truth).
- **Behavior change?** no.

#### W-API-6 — `use-store-lookups.ts` exports `useClientInstanceDirectory`

- **Category:** naming
- **Severity:** nice-to-have
- **Location(s):** `web/src/hooks/use-store-lookups.ts`
- **Observation:** file name suggests "store lookups" but exports a
  client-instance directory hook.
- **Recommendation:** rename to `use-client-instance-directory.ts`.
- **Behavior change?** no.

#### W-API-7 — `useDocumentTitle` is a 4-line file

- **Category:** simplification
- **Severity:** nice-to-have
- **Location(s):** `web/src/hooks/use-document-title.ts`
- **Observation:** 8 call sites already import it. Keeping it is fine;
  flagging because cross-cutting hooks tend to accumulate.
- **Recommendation:** leave it, or merge into a small `hooks/utils.ts`.
  Low priority.
- **Behavior change?** no.

---

### Web router / shell (W-RTR-N)

#### W-RTR-1 — `App.tsx` is a one-line indirection

- **Category:** dead code / API hygiene
- **Severity:** nice-to-have
- **Location(s):** `web/src/App.tsx`, `web/src/main.tsx`
- **Observation:** `App` is `return <AppRouter />`. Two files for zero
  work.
- **Recommendation:** delete `App.tsx`; have `main.tsx` render
  `<AppRouter />` directly.
- **Behavior change?** no.

#### W-RTR-2 — Public-routes carve-out is two literal pathname checks in `rootRoute.beforeLoad`

- **Category:** complexity
- **Severity:** nice-to-have
- **Location(s):** `web/src/router.tsx:47-84,128-145`
- **Observation:** `appLayoutRoute` has no `beforeLoad` guard — auth
  enforcement lives at root, with `/login` and `/bootstrap` as literal
  pathname exceptions.
- **Recommendation:** move the guard to `appLayoutRoute.beforeLoad`;
  public routes are siblings that never traverse it. Drop the pathname
  check.
- **Behavior change?** verify with router.test.tsx; likely no.

#### W-RTR-3 — `LogoutRouteComponent` does its real work in a `useEffect` race

- **Category:** complexity
- **Severity:** nice-to-have
- **Location(s):** `web/src/router.tsx:256-282`
- **Observation:** logout side-effect in `useEffect`; StrictMode dev
  double-mount posts logout twice.
- **Recommendation:** move side-effect into `beforeLoad`; render a
  static spinner.
- **Behavior change?** no.

#### W-RTR-4 — `AppShell.onLogout` is "optional" but every caller passes it

- **Category:** dead code / API hygiene
- **Severity:** nice-to-have
- **Location(s):** `web/src/components/app-shell.tsx:70-74,120-131`
- **Observation:** one caller, always passes the prop. The "optional"
  branch (hide the logout button) never fires.
- **Recommendation:** make required, or drop the prop and have AppShell
  navigate to `/logout` directly.
- **Behavior change?** no.

#### W-RTR-5 — `APP_ROUTE_PATHS` + invariant test for 7 NAV entries

- **Category:** complexity
- **Severity:** nice-to-have
- **Location(s):** `web/src/router.tsx:154-163,304-312`;
  `web/src/components/app-shell.tsx:29-47`.
- **Observation:** 7-row NAV table protected by a custom
  `routePathsAreConsistent` test invariant. TanStack Router's typed
  `Link` would handle this natively.
- **Recommendation:** drop `APP_ROUTE_PATHS` and use typed `<Link
  to="/secrets">`. Or live with a 404 on typo.
- **Behavior change?** no.

#### W-RTR-6 — `safeNextPath` reads `window.location.search` inside a routed component

- **Category:** complexity
- **Severity:** nice-to-have
- **Location(s):** `web/src/router.tsx:215-230,284-299`
- **Observation:** LoginRouteComponent parses `?next=` via
  `new URLSearchParams(window.location.search)` even though it's a
  TanStack Router route. A `validateSearch` would replace this.
- **Recommendation:** define `loginRoute.validateSearch` that coerces
  `next` to a same-origin path or null. Drop `safeNextPath`.
- **Behavior change?** no.

#### W-RTR-7 — `defaultPreload: "intent"` + page-level useQuery is misleading

- **Category:** complexity
- **Severity:** nice-to-have
- **Location(s):** `web/src/router.tsx:321-325`
- **Observation:** TanStack Router preloads on hover/intent, but all
  data fetching is `useQuery` at page mount — preload has nothing to
  preload.
- **Recommendation:** drop `defaultPreload: "intent"`, or move data into
  route loaders so preload actually warms the cache.
- **Behavior change?** no.

#### W-RTR-8 — Global query staleTime 30s + manual invalidates = belt-and-suspenders

- **Category:** complexity
- **Severity:** nice-to-have
- **Location(s):** `web/src/router.tsx:37-45`;
  `web/src/components/policy-viewer-dialog.tsx:48-50` overrides with
  `staleTime: 0, refetchOnMount: "always"`.
- **Observation:** PolicyViewerDialog had to override the global stale
  policy. Symptom of ad-hoc cache freshness management.
- **Recommendation:** pick `staleTime: 0` for listing queries (already
  invalidated on mutation) and keep 30s only for `whoami`/bootstrap
  status. Document the convention.
- **Behavior change?** UX-imperceptible refetches, but cache shape gets
  predictable.

#### W-RTR-9 — `Dashboard` ships em-dashes

- **Category:** dead code / API hygiene
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/dashboard.tsx`
- **Observation:** three cards showing "—" for Stores / Wrappers /
  Operators. Comment in the SPA shell: "The SPA shell is up."
- **Recommendation:** either wire real counts (one extra query),
  redirect `/` to `/secrets` (operator's actual landing place), or hide
  the dashboard route until it has content. Don't ship em-dashes.
- **Behavior change?** yes — UI changes.

---

### Web dialogs (W-DLG-N)

#### W-DLG-1 — Form+mutation+error scaffold duplicated across ~15 dialogs

- **Category:** duplication
- **Severity:** must-fix
- **Location(s):** every form-bearing dialog under
  `web/src/pages/{users,stores,client-instances,wrapper-permissions,secrets}/dialogs/`
  and `web/src/pages/secrets/libsecret/dialogs/`. Examples: `users/dialogs/create.tsx`,
  `stores/dialogs/create.tsx`, `client-instances/dialogs/edit.tsx`,
  `secrets/dialogs/create-resource.tsx`, etc.
- **Observation:** every dialog repeats:
  ```ts
  const qc = useQueryClient();
  const [submitError, setSubmitError] = useState<string | null>(null);
  const form = useForm<Values>({ resolver: zodResolver(schema), ... });
  const mut = useMutation({ mutationFn, onSuccess: async () => { await qc.invalidateQueries(...); onClose(); }, onError: (e) => setSubmitError(describeApiError(e)) });
  ```
  AND three distinct error-display patterns are used across the dialog
  family: `form.setError("root", ...)`, local `useState`,
  `setError(string)`.
- **Recommendation:** introduce `useFormDialog<S>({ schema, submit,
  invalidate, onSuccess })` returning `{ form, submit, pending, error
  }`. Pick `form.setError("root", { message: describeApiError(e) })` +
  `<Alert>{form.formState.errors.root?.message}</Alert>` as the canonical
  error UX. Saves >300 LOC and erases per-dialog cognitive tax.
- **Behavior change?** no.

#### W-DLG-2 — ConfirmDialog+mutation+invalidate wrapper duplicated 8 times

- **Category:** duplication
- **Severity:** must-fix
- **Location(s):** `users/dialogs/delete.tsx`,
  `client-instances/dialogs/{delete,set-status}.tsx`,
  `stores/dialogs/lock.tsx`,
  `secrets/dialogs/delete-secret.tsx`,
  `secrets/libsecret/dialogs/{delete-item,delete-collection,delete-alias}.tsx`
- **Observation:** the same 10-line pattern in every "delete this
  thing" dialog: a `useMutation` whose `onSuccess` invalidates and
  closes, then a `<ConfirmDialog onConfirm={() =>
  mut.mutateAsync()}/>`.
- **Recommendation:** add `useConfirmMutation({ mutationFn,
  invalidateKeys, onClose })` and/or a higher-level
  `<ConfirmActionDialog mutationFn invalidate />`. Cuts each dialog to
  ~10 lines.
- **Behavior change?** no.

#### W-DLG-3 — `EditStoreDialog` does two unrelated jobs

- **Category:** boundaries / size
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/stores/dialogs/edit.tsx`
- **Observation:** 197 lines: edit description + rotate key in one
  dialog, with a checkbox-gated confirm path and a `<KeyRevealDialog>`
  follow-up.
- **Recommendation:** split into `EditStoreDialog` (description only)
  and `RotateStoreKeyDialog` from the table action menu. Both <100 lines.
- **Behavior change?** UI re-arrangement.

#### W-DLG-4 — `EditClientInstanceDialog` re-implements form dirty tracking by hand

- **Category:** simplification
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/client-instances/dialogs/edit.tsx:77-108`
- **Observation:** computes `displayNameChanged`, `storeChanged`,
  `certChanged`, `autoCreateChanged` manually. react-hook-form already
  tracks `formState.dirtyFields`.
- **Recommendation:** use `form.formState.dirtyFields` and submit only
  changed keys.
- **Behavior change?** no.

#### W-DLG-5 — `DeleteStoreDialog` rolls its own Dialog + embeds a follow-up cleanupHint screen

- **Category:** duplication, complexity
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/stores/dialogs/delete.tsx`
- **Observation:** the only "delete" dialog that doesn't use
  `<ConfirmDialog>`. Also embeds a post-success second-screen.
- **Recommendation:** split the cleanup-hint view into a
  `<CleanupHintDialog>` opened by the parent on success. Then
  `<DeleteStoreDialog>` collapses to a `ConfirmDialog`.
- **Behavior change?** no.

#### W-DLG-6 — `KeyRevealDialog`'s preventDefault triplets

- **Category:** simplification
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/stores/dialogs/key-reveal.tsx:50-71`
- **Observation:** three `onEscapeKeyDown` / `onPointerDownOutside` /
  `onInteractOutside` handlers, all gated on `!ack`. `onInteractOutside`
  subsumes `onPointerDownOutside` in Radix.
- **Recommendation:** centralise as a `dismissable={ack}` prop on a
  shared `<NonDismissibleDialog>` or drop the redundant Radix handler.
- **Behavior change?** verify Radix subsumption; likely no.

#### W-DLG-7 — `ConfirmDialog`'s `pending` shadows caller's mutation pending

- **Category:** errors
- **Severity:** nice-to-have
- **Location(s):** `web/src/components/confirm-dialog.tsx:23,46-56`
- **Observation:** `onConfirm` typed as `() => unknown`; `ConfirmDialog`
  manages its own pending state while callers' mutation also has
  `isPending`. Two state machines for the same fact.
- **Recommendation:** accept `pending: boolean` from the caller, or
  narrow the callback to `Promise<unknown>` and keep internal pending as
  the single source.
- **Behavior change?** no.

#### W-DLG-8 — Inconsistent `onClose` vs Radix `onOpenChange` everywhere

- **Category:** naming
- **Severity:** nice-to-have
- **Location(s):** every dialog component
- **Observation:** 20+ files do `<Dialog open onOpenChange={(o) => !o
  && onClose()}>`. The translation is verbatim 20+ times.
- **Recommendation:** wrap once in `<AdminDialog open onClose />` that
  does the translation, and accept Radix's shape inside.
- **Behavior change?** no.

#### W-DLG-9 — `RevealSecretView` dialog shell duplicated across two callers

- **Category:** duplication
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/secrets/dialogs/reveal.tsx`,
  `web/src/pages/secrets/libsecret/dialogs/reveal-item-secret.tsx`
- **Observation:** body is shared; both callers wrap it in their own
  `<Dialog><DialogContent><DialogHeader><DialogTitle>...`.
- **Recommendation:** have `RevealSecretView` own the shell with `title`
  and `description` props.
- **Behavior change?** no.

---

### Web pages (W-PG-N)

#### W-PG-1 — `PolicyViewerDialog` count badge is summary of data displayed below

- **Category:** simplification
- **Severity:** nice-to-have
- **Location(s):** `web/src/components/policy-viewer-dialog.tsx:82-83,103-113`
- **Observation:** "writable permissions: N of M" sits above the same
  JSON the operator is about to read.
- **Recommendation:** move the badge to the wrapper-permissions page
  header (operator's primary view) or drop it.
- **Behavior change?** UI tweak.

#### W-PG-2 — Resource catalog mapping built independently on two pages

- **Category:** duplication
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/wrapper-permissions/page.tsx:52-59`;
  `web/src/pages/secrets/page.tsx:50-67`.
- **Observation:** both pages compute `resourceNameById = new Map<string,
  string>` from `view.data.resources` in the same shape.
- **Recommendation:** `useResourceCatalog(view.data?.resources)` hook
  returning `{ nameById, recordById }`.
- **Behavior change?** no.

#### W-PG-3 — `permissionIndex` key string format duplicated in two files

- **Category:** duplication
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/wrapper-permissions/page.tsx:77-82`
  vs `web/src/pages/wrapper-permissions/table.tsx:111`.
- **Observation:** both compose `${client_instance}|${wrapper}|${resource_id}`
  by hand. Drift would silently render every cell as Locked.
- **Recommendation:** `permissionKey(client_instance, wrapper, resource_id)`
  next to `wrapperResourceKey` in `derivations.ts`.
- **Behavior change?** no.

#### W-PG-4 — `secrets/model.ts` is a 67-line outlier module

- **Category:** boundaries / size
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/secrets/model.ts`
- **Observation:** the only `model.ts` in the codebase; the convention
  recurs nowhere.
- **Recommendation:** inline into `page.tsx`'s `useMemo`, or rename to
  `grouping.ts` to match `wrapper-permissions/derivations.ts`.
- **Behavior change?** no.

#### W-PG-5 — `wrapper-permissions/derivations.ts` mixes a type alias into a derivations module

- **Category:** boundaries / size
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/wrapper-permissions/derivations.ts`
- **Observation:** `ModeKind = WrapperApprovalMode["kind"]` is a type
  alias, not a derivation.
- **Recommendation:** inline `ModeKind` at its caller (`edit-cell.tsx`).
- **Behavior change?** no.

#### W-PG-6 — `libsecret/internal.ts` vs `index.ts` split doesn't enforce anything

- **Category:** boundaries / size
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/secrets/libsecret/{index,internal}.ts`
- **Observation:** `index.ts` exports `LibsecretTree`; `internal.ts`
  exports helpers every libsecret dialog imports directly. The
  "encapsulation" is a comment only.
- **Recommendation:** drop both files; move helpers to `tree.tsx` or to
  a flat `helpers.ts`.
- **Behavior change?** no.

#### W-PG-7 — `libsecretIdField` validation pattern duplicated in 3 dialogs

- **Category:** duplication
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/secrets/libsecret/internal.ts:9-28`;
  `create-collection.tsx`, `create-item.tsx`, `set-alias.tsx`.
- **Observation:** `id`/`label` schema portion repeated with subtle
  variations and inline error messages.
- **Recommendation:** factor a `libsecretIdField` zod helper.
- **Behavior change?** no.

#### W-PG-8 — `aliases-panel.tsx` uses undefined `--color-warning` CSS variable

- **Category:** doc / comment hygiene
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/secrets/libsecret/aliases-panel.tsx:71-72`
- **Observation:** `bg-(--color-warning)/20 text-(--color-warning)`. No
  other component uses this token; every other warning UI uses concrete
  `amber-500/30`/`amber-200` tailwind colors.
- **Recommendation:** verify the token is defined in
  `tailwind.config.*`. If not, replace with `amber-500/10` +
  `amber-200`.
- **Behavior change?** verify; possible bug.

#### W-PG-9 — Epoch arithmetic open-coded in two files

- **Category:** duplication
- **Severity:** nice-to-have
- **Location(s):** `web/src/pages/wrapper-permissions/table.tsx:184-193`;
  `web/src/pages/wrapper-permissions/dialogs/edit-cell.tsx:52-55`.
- **Observation:** `new Date(epoch * 1000)` twice; formatting via
  `formatEpochAbsolute` in one, `.toLocaleDateString()` in the other.
- **Recommendation:** `epochToDate(epoch)` helper in `lib/epoch.ts`.
- **Behavior change?** no.

#### W-PG-10 — `EntityCreator` label format "client {name}" is bare-prefix and inconsistent

- **Category:** naming
- **Severity:** nice-to-have
- **Location(s):** `web/src/lib/creator.ts:8-18`
- **Observation:** operator-facing string concatenates `"client "` +
  name/id. Other places render just the name.
- **Recommendation:** either drop the prefix and let the caller decorate
  with a badge (consistent with the wrapper-permissions table), or
  return `{ label, kind }` and decorate at the renderer.
- **Behavior change?** UI tweak.

---

## Ticket plan

**Shape:** one epic + 18 child task tickets (volume justifies the epic
structure; flat would be unmanageable). Numeric prefixes start at `1000_`.

The epic file goes alongside the task files in the same `01_todo/` move
(executed by the follow-up session).

```
tickets/01_todo/
  epic_web_backend_cleanup_2026-05.md
  1000_story_seam_naming_and_contract.md
  1010_task_rename_storeName_to_storeId.md
  1020_task_lock_response_to_shared_protocol.md
  1030_task_logout_204_and_drop_emptyobject.md
  1040_task_narrow_secret_list_slot_schema.md
  1050_task_structured_store_delete_cleanup_hint.md
  1060_task_atomic_pair_with_new_resource.md
  1100_task_runtime_api_into_response_unification.md
  1110_task_admin_api_router_dedup.md
  1120_task_admin_api_helpers_consolidation.md
  1130_task_admin_api_mutate_collection_typed_errors.md
  1140_task_admin_api_pub_tightening_and_dto_locations.md
  1150_task_drop_client_instance_permissions_visibility.md
  1200_task_collapse_json_dir_store.md
  1210_task_collapse_loaded_store_record.md
  1220_task_split_admin_runtime_state.md
  1230_task_config_collapse_options.md
  1240_task_lib_rs_pub_cleanup.md
  1250_task_runtime_api_register_decomposition.md
  1300_task_use_form_dialog_and_confirm_mutation.md
  1310_task_collapse_api_client_wrappers.md
  1320_task_router_guard_simplification.md
  1330_task_lazy_libsecret_tree.md
  1340_task_web_dialog_shell_unification.md
  1350_task_web_misc_simplifications.md
  1360_task_doc_comment_sweep.md
```

Numbering blocks:
- `1000–1099` seam (cross-component contract changes)
- `1100–1199` admin_api refactors
- `1200–1299` backend core refactors
- `1300–1399` web SPA refactors

Each task ticket below carries: title, one-paragraph description, the
findings it resolves, and the acceptance check.

---

### epic_web_backend_cleanup_2026-05.md

Title: **Web + backend cleanup pass — first-release polish**

Body:

> This epic bundles 18 task tickets emitted from the 2026-05-24
> sweeping review (`tickets/_review/web_backend_seam_2026-05-24.md`).
> Scope: the admin SPA at `web/`, the `backend-backendd` crate, and
> the OpenAPI seam between them. Posture is greenfield-aggressive
> (wire renames, NixOS-module changes, and `pub` tightening are all
> in-bounds; backwards compatibility is not a concern).
>
> The epic does not change any security boundary; tickets that touch
> auth/session/CSRF code carry the VM-test acceptance check.
>
> Story `1000_story_seam_naming_and_contract` carves out the
> cross-component contract changes; the rest sit in two roughly
> independent threads (admin_api/runtime_api/core; web SPA).

Acceptance: every child ticket is closed with its own acceptance check
green, and a final `cargo test --workspace`, `cargo clippy --all-targets
--workspace -- -D warnings`, `cd web && npm test`, and `nix build
.#checks.x86_64-linux.vm-test` pass.

---

### 1000_story_seam_naming_and_contract.md

Title: **Seam: naming and wire-format cleanup across SPA ↔ backend**

Body:

> Six small wire-shape or contract changes that touch both halves of
> the codebase. Sequence them first so the per-side refactors that
> follow consume the new shapes.

Resolves: S-1, S-2, S-3, S-4, S-5, S-6, S-9, S-10 (S-7 and S-8 are
performance/clarity-only and land in 1320/1330).

Acceptance (per child ticket):
- `cargo test --workspace`
- `cd web && npm test`
- After re-running `npm run codegen` (or whatever pulls openapi.json
  into schema.ts) the SPA still type-checks.

---

### 1010_task_rename_storeName_to_storeId.md

Title: **Rename SPA `storeName` to `storeId` end-to-end**

Body:

> The SPA threads `storeName` through every per-store page, dialog,
> and hook, but the value is always the backend's `id` field. Rename
> the variable, prop, and hook to `storeId` everywhere it appears.
> No wire shape changes — backend already names the path parameter
> `id`. Touches ~30 files but is purely cosmetic for the operator.

Resolves: S-1.

Acceptance:
- `cd web && npm run build && npm test`
- `cd web && npx tsc --noEmit` (covered by build)
- Manually walk through `/stores`, `/secrets`, `/wrapper-permissions`
  selecting different stores; verify the URL `?store=` and selector
  stay in sync.

---

### 1020_task_lock_response_to_shared_protocol.md

Title: **Move `StoreLockResponse` to `shared-protocol::service`**

Body:

> The inline `pub struct StoreLockResponse { ok, was_locked }` lives
> in `admin_api/stores/lifecycle.rs` while every other `Admin*Response`
> is defined in `shared-protocol::service`. Move it; rename to
> `AdminStoreLockResponse` for naming symmetry; regenerate `schema.ts`;
> update SPA call sites.

Resolves: S-2.

Acceptance:
- `cargo test --workspace`
- OpenAPI drift test (`cargo test --test openapi_export`) green
- `cd web && npm test && npm run build`

---

### 1030_task_logout_204_and_drop_emptyobject.md

Title: **Return 204 from logout; delete `EmptyObject`**

Body:

> The `#[utoipa::path]` annotation on `handle_admin_logout` declares
> 204 but the handler returns `Json<EmptyObject>` (which is 200 with
> `{}`). Return `StatusCode::NO_CONTENT` directly; delete
> `EmptyObject`. SPA already tolerates either via `unwrap`.

Resolves: S-3.

Acceptance:
- `cargo test -p backend-backendd`
- `cargo test -p integration-tests`
- `cd web && npm test`

---

### 1040_task_narrow_secret_list_slot_schema.md

Title: **Narrow `slot` schema for `/v1/admin/stores/{id}/secrets`**

Body:

> The secrets list endpoint never emits `libsecret` entries, but the
> SPA defensively filters them in `groupSecretEntries`. Narrow the
> OpenAPI schema for this endpoint's response slot field to
> `enum: ["env","files"]` (define a `KvSecretSlot` newtype on the
> backend if needed). Delete the SPA's defensive filter and the
> stale comment ("legacy flat-KV bridge").

Resolves: S-4 and one entry from B-DOC-2.

Acceptance:
- `cargo test -p backend-backendd`
- `cd web && npm test && npm run build`

---

### 1050_task_structured_store_delete_cleanup_hint.md

Title: **Return structured `cleanup_hint` from store-delete**

Body:

> The store-delete handler currently ships a freeform prose string
> the SPA dumps into `<pre>`. Change the wire shape to
> `cleanup_hint: { data_dir: String, command: String }` and have
> the SPA render the prose. Backend keeps the data; UX text moves
> to the SPA. Refactor `DeleteStoreDialog` along the way (see
> 1340).

Resolves: S-5.

Acceptance:
- `cargo test -p backend-backendd`
- `cd web && npm test && npm run build`

---

### 1060_task_atomic_pair_with_new_resource.md

Title: **Add `POST /v1/admin/stores/{id}/pairs/with-new-resource`**

Body:

> The SPA's `CreatePairDialog` orchestrates "create resource → check
> dup → create pair" as two backend calls with no rollback; failure
> after step 1 orphans the new resource. Add a single backend
> endpoint that does both in one transaction; update the SPA to use
> it. Drop the SPA's `existingPairs` pre-flight scan; let the
> backend's stable error code feed `describeApiError`.

Resolves: S-6, S-9.

Acceptance:
- `cargo test -p backend-backendd`
- `cargo test -p integration-tests`
- `cd web && npm test && npm run build`

---

### 1100_task_runtime_api_into_response_unification.md

Title: **Unify runtime_api response shape on `axum::Response`**

Body:

> The runtime API returns `hyper::Response<Full<Bytes>>` via hand-
> built builders, forcing ~30 lines of `match load_…(store)` +
> `error_response(...)` per handler. Build a `RuntimeError` with
> `IntoResponse` mirroring `AdminError`'s envelope. Handlers return
> `Result<impl IntoResponse, RuntimeError>` and use `?`. Delete
> `runtime_api/responses.rs`. **Security:** wire envelope identical
> on the wire (verified by integration tests + VM test).

Resolves: B-RT-1, B-RT-2, B-RT-8.

Quotes from `CLAUDE.md` / `docs/security.md` this must not violate:
- "No fallback modes — if a security mechanism fails, the wrapper
  aborts."
- The runtime API's `Deny { reason }` shape must be unchanged.

Acceptance:
- `cargo test --workspace`
- `cargo test -p integration-tests`
- `nix build .#checks.x86_64-linux.vm-test`
- Wire-shape JSON snapshot tests in `runtime_api/*/tests.rs` stay
  green.

---

### 1110_task_admin_api_router_dedup.md

Title: **Collapse `admin_router_and_openapi` + drop the BC wrapper**

Body:

> `admin_router_and_openapi` exists only for `admin_router(state).0`
> and has a misleading "Backwards-compatible wrapper" docstring.
> Build the `OpenApiRouter` once via a shared `build_admin_openapi_router`
> helper; `admin_router` applies layers and takes
> `.split_for_parts().0`, `admin_openapi_spec` takes `.1`. Delete the
> two-shape construction. Reconsider `openapi::admin_openapi_spec`
> forwarder: either `pub use` or delete (B-ADM-2).

Resolves: B-ADM-1, B-ADM-2, B-DOC-2 (one entry).

Acceptance:
- `cargo test -p backend-backendd`
- `cargo test --test openapi_export`

---

### 1120_task_admin_api_helpers_consolidation.md

Title: **Consolidate admin_api shared helpers**

Body:

> Pull out and unify:
> - `validate_password_or_bad_request` used by 4 handlers (B-ADM-4)
> - `result_label<T>(r: &Result<T, AdminError>) -> &'static str` in
>   `admin_api::audit`, replacing `audit_result` and 4 open-coded ternaries (B-ADM-7)
> - `saturating_u32` promoted to a `util` module (B-ADM-6)
> - `policy_permission_dependents(predicate)` next to
>   `resource_secret_dependents` (B-ADM-5)
> - `IntoInternal` trait giving `.into_internal(ctx)` on
>   `anyhow::Error` (B-ADM-25)
> - rename `body_b64.rs` → fold into `util.rs` (B-ADM-24)
>
> Also clean up `parse_slot` (B-ADM-15), the `force` query parsing
> (B-ADM-16), and redundant `#[serde(default)]` on `Option<T>` fields
> (B-ADM-17). Compress `reencrypt_into`'s repeated cleanup-on-error
> block (B-ADM-14).

Resolves: B-ADM-4, B-ADM-5, B-ADM-6, B-ADM-7, B-ADM-14, B-ADM-15,
B-ADM-16, B-ADM-17, B-ADM-24, B-ADM-25.

Acceptance:
- `cargo test --workspace`
- `cargo clippy --all-targets --workspace -- -D warnings`

---

### 1130_task_admin_api_mutate_collection_typed_errors.md

Title: **`mutate_collection`: typed errors, drop the diff**

Body:

> Change `mutate_collection`'s closure to return `Result<T,
> UserMutationError>` instead of `anyhow::Result<T>`; return
> `MutateCollectionError { User(_) | Io(_) }` from the helper.
> Handlers `match` cleanly; IO maps to 500 instead of being
> downcast-swallowed to 400. Drop the O(n) before/after diff and
> let the closure (or the handler) declare which records to
> persist (B-ADM-8). Normalise `audit_user_mutation`'s success
> extractor to a `String` (B-ADM-10).

Resolves: B-ADM-8, B-ADM-9, B-ADM-10.

Acceptance:
- `cargo test --workspace`
- New unit test: IO failure during `save_user` surfaces as 500, not 400.

---

### 1140_task_admin_api_pub_tightening_and_dto_locations.md

Title: **Tighten admin_api `pub` surface and move inline DTOs**

Body:

> Demote `pub mod auth` / `pub mod bootstrap` and inline DTOs
> (`LoginRequest`, `LoginResponse`, `WhoamiResponse`, `EmptyObject`,
> `BootstrapRequest`/`Response`/`StatusResponse`) to `pub(crate)`.
> Hoist inline `use underwrap_protocol::…` statements from function
> bodies to the file's top-level `use` block (B-ADM-27). Tighten
> `AdminError::new` to private (B-ADM-11). Decide on the
> `store_not_found` helper symmetry (B-ADM-12).

Resolves: B-ADM-3, B-ADM-11, B-ADM-12, B-ADM-27.

Acceptance:
- `cargo build -p backend-backendd`
- `cargo test --workspace`

---

### 1150_task_drop_client_instance_permissions_visibility.md

Title: **Make policy cleanup visibility-correct for locked stores**

Body:

> `policy::drop_client_instance_permissions` currently does `Err(_)
> => return Ok(())` for both `Locked` and `NotFound`. When a store
> is locked at the moment an instance is remapped or deleted,
> orphaned permissions stay on disk silently. Match on
> `RegistryLookupError` explicitly. For `Locked`: emit a structured
> `tracing::warn!` + audit line and consider returning an error so
> the caller can refuse. For `NotFound`: `tracing::info!` + `Ok(())`.

Resolves: B-ADM-20.

`docs/security.md` posture: this strengthens an audit-trail
invariant, doesn't weaken any boundary. Add an integration test
that asserts the warning fires when permissions cleanup is skipped.

Acceptance:
- `cargo test -p integration-tests`
- `nix build .#checks.x86_64-linux.vm-test`

---

### 1200_task_collapse_json_dir_store.md

Title: **Lift `json_dir_store<T>` generic for the three on-disk registries**

Body:

> `users.rs`, `store_registry.rs`, `runtime_clients.rs` each
> implement the same `read_dir + filter + sort + uniqueness scan +
> tempfile-write + sync_all + persist + NotFound-tolerant delete`
> pattern. Lift to a `json_dir_store<T: DirRecord>` module exposing
> `load_dir`, `save`, `delete`. Each domain module shrinks to ~30
> lines of trait impl. Drop the three `is_valid_*_id` aliases of
> `ids::is_valid_simple_id` (B-COR-4). Rename to `is_valid_id`.

Resolves: B-COR-2, B-COR-4.

Acceptance:
- `cargo test --workspace`
- Round-trip test: load/save/delete via the generic matches the
  pre-refactor behaviour byte-for-byte (snapshot test).

---

### 1210_task_collapse_loaded_store_record.md

Title: **Drop `LoadedStoreRecord`; collapse parallel maps to `StoreEntry`**

Body:

> Delete the zero-value `LoadedStoreRecord` newtype (B-COR-3).
> Collapse `StoreRegistry { state, records }` to a single
> `HashMap<String, StoreEntry>` where `StoreEntry { record:
> StoreRecord, state: StoreState, rotation_lock: Mutex<()> }`.
> Pulls the rotation lock onto the entry (B-ADM-18) so it dies
> with the store. Adds `admin_store_state(id)` helper used by the
> store handlers (B-ADM-13). Returns `()` from `unlock` (B-COR-6);
> deletes `is_locked` and `StoreState::is_locked` (B-COR-5,
> B-COR-15).

Resolves: B-ADM-13, B-ADM-18, B-COR-3, B-COR-5 (partial), B-COR-6,
B-COR-15.

Acceptance:
- `cargo test --workspace`
- `nix build .#checks.x86_64-linux.vm-test`

---

### 1220_task_split_admin_runtime_state.md

Title: **Split `AdminRuntimeState` god-struct**

Body:

> Carve out `SharedState { stores, client_instances }` and
> `AdminState { session_store, auth_backend, csrf_config,
> session_lifetime_secs, cookies_secure, bootstrap_lock,
> user_collection_lock, allowed_origins, users_dir, bootstrap_file,
> stores_data_root, rotation_locks }`. The listener carries
> `Arc<SharedState>` and `Option<Arc<AdminState>>`. Rename the
> umbrella type to `ListenerState` (or equivalent). Wire
> `csrf_config` from `cookies_secure` (S-10). Move `Principal` to
> `admin_api::csrf` (B-COR-8).

Resolves: B-COR-7, B-COR-8, S-10.

Acceptance:
- `cargo test --workspace`
- `nix build .#checks.x86_64-linux.vm-test`

---

### 1230_task_config_collapse_options.md

Title: **Collapse `Option<T>` config fields to required + default**

Body:

> `allowed_origins: Option<Vec<String>>` → `Vec<String>` with
> `#[serde(default)]` (B-CFG-1). `session_lifetime_secs: Option<u64>`
> → `u64` with `#[serde(default = "...")]` (B-CFG-2). Four
> `Option<PathBuf>` dirs + resolver methods → `PathBuf` with
> `#[serde(default = "...")]` each (B-CFG-3). The four `*_dir()`
> resolver methods disappear.

Resolves: B-CFG-1, B-CFG-2, B-CFG-3.

`examples/config.yaml` and the NixOS module's defaults stay; the
schema reads cleaner.

Acceptance:
- `cargo test --workspace`
- `nix build .#checks.x86_64-linux.vm-test` (verifies the module's
  emitted YAML still deserializes).

---

### 1240_task_lib_rs_pub_cleanup.md

Title: **Strip `lib.rs` re-exports + tighten module visibility**

Body:

> Delete the `pub use crate::admin_api::auth::{...}` and
> `pub use crate::store_registry::{...}` re-exports — workspace
> grep finds zero external consumers. Demote `pub mod auth`,
> `pub mod web`, `pub mod tls`, `pub mod users` to `pub(crate) mod`.
> Rewrite the "External consumers… reach for these types via the
> crate root" comment to match reality (consumers go through
> `BackendConfig`, `cli::*`, `server::run`, `openapi::*`,
> `store_registry::save_store_record`, `runtime_clients::*`).
> Delete `cert_fingerprint_from_pem_file` (B-COR-5). Delete the
> manual `Debug for UserCollection` (B-COR-9). Decide on
> `load_bootstrap_sentinel` (B-COR-12: delete or surface).
> Inline or relocate `now_epoch_secs` and `new_user_id`
> (B-COR-13). Tighten `Body<T>` field visibility (B-COR-14).
> Drop `Body::body_then_into_response` / drop `generate_resource_id`
> (B-RT-5).

Resolves: B-COR-1, B-COR-5 (remainder), B-COR-9, B-COR-10
(`recover_user` cleanup), B-COR-12, B-COR-13, B-COR-14, B-RT-5.

Acceptance:
- `cargo build -p backend-backendd`
- `cargo build -p integration-tests`
- `cargo test --workspace`
- `cargo clippy --all-targets --workspace -- -D warnings`

---

### 1250_task_runtime_api_register_decomposition.md

Title: **Decompose `handle_runtime_wrapper_register`**

Body:

> Carve the 240-line handler into:
> - `compute_register_diff(body, &resources, &pairs) -> RegisterDiff`
> - `apply_auto_create(&mut resources, &mut pairs, &diff, creator) ->
>   (Vec<String>, Vec<RuntimeWrapperRegisterPair>)`
> - `seed_locked_permissions(policy, body, resolved, principal) -> bool`
>   (lifted from the inline closure)
> - a driver dispatching the three outcomes (no-op / refusal /
>   commit).
>
> Drop `runtime_api::principal`'s redundant existence check
> (B-RT-7). Add the cross-reference comment to
> `validate_runtime_client_mappings` pointing at the admin
> delete handler (B-RT-6).

Resolves: B-RT-3, B-RT-4, B-RT-5, B-RT-6, B-RT-7.

Acceptance:
- `cargo test --workspace`
- `cargo test -p integration-tests` (wire-format unchanged)
- `nix build .#checks.x86_64-linux.vm-test`

---

### 1300_task_use_form_dialog_and_confirm_mutation.md

Title: **`useFormDialog` + `useConfirmMutation` consolidation**

Body:

> Add two hooks (and optional wrapper components):
>
> ```ts
> useFormDialog<S extends z.ZodTypeAny>({
>   schema, defaultValues, submit, invalidate, onSuccess
> }): { form, submit, error }
>
> useConfirmMutation({ mutationFn, invalidateKeys, onClose }):
>   { confirm, pending, error }
> ```
>
> Pick `form.setError("root", { message: describeApiError(e) })`
> + `<Alert>{form.formState.errors.root?.message}</Alert>` as the
> single error-display pattern. Convert every form/confirm dialog
> under `web/src/pages/*/dialogs/` and
> `web/src/pages/secrets/libsecret/dialogs/` to use the hooks.

Resolves: W-DLG-1, W-DLG-2; partially resolves W-DLG-7
(`ConfirmDialog.pending`).

Acceptance:
- `cd web && npm test`
- `cd web && npm run build`
- Manual walk-through: every create/edit/delete dialog still
  closes on success, displays the same error on failure, and
  shows pending state.

---

### 1310_task_collapse_api_client_wrappers.md

Title: **Collapse `api/client.ts` wrapper boilerplate**

Body:

> Drop the ~500 lines of one-line endpoint wrappers in
> `web/src/api/client.ts`. Either inline `client.GET(...)` /
> `unwrap(...)` at call sites, or expose `api.GET / api.POST /
> api.PUT / api.DELETE` that wrap `unwrap` + the client once.
> Delete the ~12 local-only `type` aliases (W-API-2).
> Replace `_resetCsrfTokenForTesting` with `vi.resetModules()` in
> tests (W-API-3). Change `unwrap` signature to `Promise<T |
> undefined>` for honest 204 handling (W-API-4).

Resolves: W-API-1, W-API-2, W-API-3, W-API-4.

Acceptance:
- `cd web && npm test`
- `cd web && npm run build` (typecheck)

---

### 1320_task_router_guard_simplification.md

Title: **Router shell + auth guard + login `search` schema**

Body:

> - Move auth guard from `rootRoute.beforeLoad` to
>   `appLayoutRoute.beforeLoad`; drop the public-routes pathname
>   carve-out (W-RTR-2).
> - Move logout side-effect into `logoutRoute.beforeLoad`;
>   `LogoutRouteComponent` becomes a static spinner (W-RTR-3).
> - Make `AppShell.onLogout` required (W-RTR-4).
> - Drop `APP_ROUTE_PATHS` constraint; use typed `<Link>` everywhere
>   (W-RTR-5).
> - Define `loginRoute.validateSearch` for `?next=`; delete
>   `safeNextPath` (W-RTR-6).
> - Drop `defaultPreload: "intent"` (W-RTR-7).
> - Delete `App.tsx`; render `<AppRouter />` from `main.tsx`
>   (W-RTR-1).
> - Decide global query strategy: `staleTime: 0` for listing queries
>   (already invalidated on mutation) and 30s for `whoami`/bootstrap
>   only; drop the PolicyViewerDialog's per-query override (W-RTR-8).
> - Either wire real `Dashboard` counts or redirect `/` → `/secrets`
>   (W-RTR-9).
> - Drop the `useStoreSelector` URL-write effect (W-API-5).
> - Rename `use-store-lookups.ts` → `use-client-instance-directory.ts`
>   (W-API-6).

Resolves: W-API-5, W-API-6, W-RTR-1, W-RTR-2, W-RTR-3, W-RTR-4,
W-RTR-5, W-RTR-6, W-RTR-7, W-RTR-8, W-RTR-9.

Acceptance:
- `cd web && npm test`
- `cd web && npm run test:e2e` (Playwright, inside `nix develop`)
- `cd web && npm run build`

---

### 1330_task_lazy_libsecret_tree.md

Title: **Lazy `LibsecretTree` + dedup query keys**

Body:

> `LibsecretTree` mounts under `<details>` per resource and fires a
> `useQuery` regardless of whether the operator expanded it. For a
> store with N resources that's N round-trips on page load. Either
> (a) gate the query on `<details>` open state, or (b) fold the
> libsecret data into the existing `secrets-view` aggregate so
> the page is one round-trip total. Pick (b) if greenfield.
>
> Also dedup the policy-viewer query (S-7): pass `permissions` from
> the wrapper-permissions page into PolicyViewerDialog instead of
> fetching the smaller endpoint.

Resolves: S-7, S-8.

Acceptance:
- `cd web && npm test`
- Manual check with a store carrying ≥5 resources: one round-trip
  visible in DevTools instead of N+.

---

### 1340_task_web_dialog_shell_unification.md

Title: **Web dialog shell unification + EditStore split + reveal shell**

Body:

> - Wrap every dialog in `<AdminDialog open onClose>` that does the
>   Radix `onOpenChange` translation once (W-DLG-8).
> - Split `EditStoreDialog` into `EditStoreDialog` (description only)
>   and `RotateStoreKeyDialog` triggered from the table action menu
>   (W-DLG-3).
> - Use `form.formState.dirtyFields` in `EditClientInstanceDialog`
>   instead of manual diff (W-DLG-4).
> - Convert `DeleteStoreDialog` to `<ConfirmActionDialog>` and split
>   the cleanup-hint follow-up into `<CleanupHintDialog>` (W-DLG-5).
>   Pair with the structured `cleanup_hint` change (1050).
> - Centralise `KeyRevealDialog`'s preventDefault behaviour
>   (W-DLG-6); drop redundant Radix handler.
> - Have `RevealSecretView` own the Dialog shell with `title`/
>   `description` props (W-DLG-9).

Resolves: W-DLG-3, W-DLG-4, W-DLG-5, W-DLG-6, W-DLG-7 (remainder),
W-DLG-8, W-DLG-9.

Acceptance:
- `cd web && npm test`
- `cd web && npm run test:e2e`

---

### 1350_task_web_misc_simplifications.md

Title: **Page-level dedup + naming + tiny cleanups**

Body:

> - `useResourceCatalog(view.data?.resources)` hook used by
>   wrapper-permissions and secrets pages (W-PG-2).
> - `permissionKey(client_instance, wrapper, resource_id)` helper
>   in `derivations.ts`; use on both sides (W-PG-3).
> - Inline `secrets/model.ts` into `secrets/page.tsx` or rename to
>   `grouping.ts` (W-PG-4).
> - Inline `ModeKind` from `derivations.ts` (W-PG-5).
> - Drop `libsecret/{index,internal}.ts` split; flatten to a single
>   `helpers.ts` (W-PG-6).
> - Factor a `libsecretIdField` zod helper (W-PG-7).
> - Resolve or replace `--color-warning` token (W-PG-8).
> - `epochToDate(epoch)` helper in `lib/epoch.ts` (W-PG-9).
> - Move `PolicyViewerDialog` writable-count badge to the page
>   header (W-PG-1).
> - Decide `EntityCreator` label format (W-PG-10).

Resolves: W-PG-1, W-PG-2, W-PG-3, W-PG-4, W-PG-5, W-PG-6, W-PG-7,
W-PG-8, W-PG-9, W-PG-10.

Acceptance:
- `cd web && npm test && npm run build`

---

### 1360_task_doc_comment_sweep.md

Title: **Backend WHAT-comment + stale-rustdoc sweep**

Body:

> Delete WHAT-comments narrating the next line and stale rustdoc
> paragraphs across the backend (B-DOC-1, B-DOC-2 remainder). Sites:
> `admin_api/mod.rs:90-110`, `admin_api/csrf.rs:138-140`,
> `admin_api/secrets.rs:332-334`, `admin_api/auth/bootstrap.rs:170-171`,
> `admin_api/libsecret.rs:58,144,197,330,499`, `admin_api/audit.rs:10-12`,
> `admin_api/mod.rs:166-168`. Also: collapse duplicated logging
> in `runtime_api/mod.rs:65-68` (B-RT-8); drop `tls.rs` instance
> field redundancy (B-COR-17); revisit `web.rs` SPA-unavailable
> HTML — replace with `compile_error!` if `web/dist/` is missing
> at build time (B-COR-16); reconsider per-request `info!` log in
> `admin_api/csrf.rs:169-174` (B-ADM-26).
>
> This is a final simplification pass on the touched files. Apply
> the category 7a lens (no defensive code, no needless
> abstractions, no impossible-state error paths).

Resolves: B-DOC-1, B-DOC-2 (remainder), B-RT-8, B-COR-16, B-COR-17,
B-ADM-26.

Acceptance:
- `cargo test --workspace`
- `cargo clippy --all-targets --workspace -- -D warnings`

---

## Next step

Approve the plan and start a fresh session — it will pick this report
up from `tickets/_review/` and create the tickets under
`tickets/01_todo/`, then execute them via `underwrap-ticket-workflow`,
one at a time. Recommended execution order matches the numeric prefixes
(seam first; then the two backend threads in parallel; then the SPA
thread; finish with the doc/comment sweep).
