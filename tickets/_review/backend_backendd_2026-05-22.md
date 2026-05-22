# Refinement review: backend-backendd

Date: 2026-05-22
Reviewer session: claude-opus-4-7[1m]
Scope: the `backend-backendd` crate at `crates/backend-backendd/` — the central
HTTPS service that holds secrets, hosts the admin SPA + JSON API, hosts the
runtime mTLS API for frontend daemons, and owns the policy / resources /
pairs / users / client-instances state. Dependents are
`integration-tests`, the `ts_export` binding test, and the `underwrap-backendd`
binary; dependencies are `shared-protocol` and `shared-store`. The crate is
~9k lines across 62 files. Existing tickets under `tickets/01_todo/` and
`tickets/02_inprogress/` are both empty, so no overlap to cross-reference.

## Summary

- 20 findings across 8 categories.
- 12 must-fix, 6 nice-to-have, 2 out-of-scope.
- Proposed ticket shape: **epic + numbered tasks** — 8 in-scope tasks
  (several non-trivial moves) plus 2 repo-wide sweep tasks. See "Ticket
  plan" below.

The crate is in reasonable shape after the recent `epic_backend_modularization`
work (sibling `tests.rs`, split admin_api modules, unified audit shape).
What remains is: pre-first-release schema-version baggage that the
"Posture" section of the skill explicitly flags as dead weight; fragile
`msg.contains(...)` error-reason matching across user CRUD and one
runtime path; an `_endpoint`/inner-handler double pattern that lingers in
every admin module long after the test reason for it has narrowed;
several truly-unused parameters (`_state` audit shims, `_now_epoch`,
`_stores`); duplication between two id validators and four hand-rolled
hex encoders; lingering ticket / epic pointers in source; the SCM-rights
SPA / runtime path still works fine and is left alone.

## Findings

### F-001 — Pre-first-release schema-version stamps and `serde(default)` tolerance must go

- **Category:** 9 (Greenfield cleanups)
- **Severity:** must-fix
- **Scope:** in-component
- **Location(s):**
  - `crates/backend-backendd/src/users.rs:27-31` — `COLLECTION_VERSION: u32 = 2`
    and `STATE_FILE_VERSION: u32 = 2`. The header comment literally says
    "Bump when the on-disk shape changes incompatibly."
  - `crates/backend-backendd/src/users.rs:194-216` — `UserStateDocument`
    carries a `#[serde(default = "default_state_file_version")] version: u32`
    field and a `default_state_file_version` helper that exists solely to
    tolerate older shapes.
  - `crates/backend-backendd/src/users.rs:296-300, 305-310` — load/save
    each `ensure!` the version equals the constant; no `match version {
    1 => …, 2 => … }` branch, just an early-fail sanity check.
  - `crates/backend-backendd/src/runtime_clients.rs:51-69` —
    `ClientInstancesDocument.version: u32` with
    `default_client_instances_version() = 1` and an `ensure!` in
    `load_client_instances` (lines 117-121).
  - `crates/backend-backendd/src/runtime_clients.rs:22-23` —
    `CurrentCredentialRecord.issued_at` is
    `#[serde(default, skip_serializing_if = "Option::is_none")]` with
    the comment "Optional so older test fixtures can omit it cleanly."
  - `crates/backend-backendd/src/runtime_clients.rs:46-47` —
    `auto_create_wrapper_assets` is `#[serde(default)]` with the
    comment "Pre-existing records on disk that predate this field
    default to `false`."
  - `crates/shared-protocol/src/entities.rs` — `ENTITY_SCHEMA_VERSION`
    used at `crates/backend-backendd/src/storage/documents.rs:45-50,
    77-82`. Same pattern: load → `ensure! version ==
    ENTITY_SCHEMA_VERSION` → no migration arm.
  - `crates/backend-backendd/src/store_registry.rs:185-189, 230` —
    `StoreRegistryDocument.version` with the same load/save assertion
    pair.
- **Observation:** Every persisted document this crate owns carries a
  `version: u32` discriminator, an `ensure!` that the field equals a
  hard-coded constant, and at least one `serde(default)` field whose
  comment says "older fixtures might omit this." None of the load paths
  branch on the version. Per `CLAUDE.md` "Release posture" + the
  skill's posture section, this is exactly the shape that must be
  removed before the first release: "schema-version fields … or
  `match version { 1 => ..., 2 => ... }` branches when only one shape
  was ever live" + "`serde(default)` / `Option<T>` fields that exist
  only to tolerate older serialized blobs."
- **Recommendation:** Delete the `version` field from
  `UserStateDocument`, `ClientInstancesDocument`,
  `StoreRegistryDocument`, and (via the protocol crate)
  `ResourcesDocument` / `PairsDocument`. Drop the
  `default_state_file_version` / `default_client_instances_version`
  helpers and the matching `ensure!` assertions. For
  `CurrentCredentialRecord.issued_at`: keep `Option<u64>` (the value is
  legitimately unknown for self-imported certs) but drop the
  `serde(default)` and the "older test fixtures" comment — make the
  test fixtures pass it explicitly. For `auto_create_wrapper_assets`:
  drop the `serde(default)` and update the protocol struct so the
  field is required on disk; backend creates the field in every
  `ClientInstanceRecord` it writes today anyway.
- **Behavior change?** no — the assertions and defaults exist only to
  paper over states that were never deployed. Removing them is a pure
  refactor against current on-disk shapes the test fixtures already
  produce.
- **Related tickets:** none.

### F-002 — Audit helpers carry an unused `_state` parameter

- **Category:** 4 (Dead code & API hygiene)
- **Severity:** must-fix
- **Scope:** in-component
- **Location(s):**
  - `crates/backend-backendd/src/admin_api/policy.rs:53-54` —
    `write_admin_permission_audit(_state: &AdminRuntimeState, ...)`.
  - `crates/backend-backendd/src/admin_api/users.rs:43-44` —
    `write_admin_user_audit(_state: &AdminRuntimeState, ...)`.
  - `crates/backend-backendd/src/admin_api/bootstrap.rs:229-234` —
    `audit_bootstrap_attempt(_state: &AdminRuntimeState, ...)`.
  - `crates/backend-backendd/src/admin_api/stores/mod.rs:264-272` —
    `write_admin_store_audit(_state: &AdminRuntimeState, ...)`.
- **Observation:** Four audit helpers take `&AdminRuntimeState` as their
  first argument and immediately rename it to `_state`. None of them
  consult state — they just hand the operator/resource strings to
  `log_admin_event`. Adding the parameter forces every call site to
  thread `state` through even when it has nothing to say. (Compare
  with `client_instances.rs:27-36` and `resources.rs:29-38`, which
  *don't* take state and read identically.)
- **Recommendation:** Drop `_state` from all four helpers and from
  every call site. Ticket 1230's recent audit-shape unification already
  collapsed the per-subdomain wrappers into a consistent signature;
  this is the trailing dead parameter from that pass.
- **Behavior change?** no.
- **Related tickets:** ticket 1230 (already in `tickets/03_done/`).

### F-003 — `overview.rs` carries an unused `_now_epoch` parameter and computes the value just to pass it

- **Category:** 4 (Dead code & API hygiene)
- **Severity:** must-fix
- **Scope:** in-component
- **Location(s):**
  - `crates/backend-backendd/src/admin_api/overview.rs:21-25` —
    `admin_ui_overview_for_store(_, _, _now_epoch: u64)`.
  - `crates/backend-backendd/src/admin_api/overview.rs:64-68, 85` —
    caller computes `now_epoch` and passes it in.
- **Observation:** `admin_ui_overview_for_store` accepts a `now_epoch`
  it never reads (the `_` prefix is the giveaway). The caller still
  computes it via `SystemTime::now()`. The previous "UnlockedUntil"
  bucket may have used it; today the function only buckets by mode
  variant and doesn't need any clock value.
- **Recommendation:** Remove the parameter and the caller's unused
  `now_epoch` computation.
- **Behavior change?** no.
- **Related tickets:** none.

### F-004 — `admin_principal(_stores: &[&str])` in `test_support` is always called with `&[]`

- **Category:** 4 (Dead code & API hygiene)
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):**
  - `crates/backend-backendd/src/test_support.rs:444-449` — definition.
  - `crates/backend-backendd/src/test_support.rs:528, 629`,
    `client_instances/tests.rs:17,112,219,301`,
    `pairs/tests.rs:11,56`, `secrets/tests.rs:12`, and ~15 more — every
    caller passes `&[]`.
- **Observation:** The `_stores` parameter was presumably for a per-test
  scoping concept that has since gone away.
- **Recommendation:** Drop the parameter; update the call sites.
- **Behavior change?** no.
- **Related tickets:** none.

### F-005 — `msg.contains(...)` error-reason sniffing across user CRUD (and one runtime path)

- **Category:** 3 (Error handling) + 6 (Complexity)
- **Severity:** must-fix
- **Scope:** in-component
- **Location(s):**
  - `crates/backend-backendd/src/admin_api/users.rs:187-205` (create),
    `:252-277` (update), `:336-355` (reset_password), `:388-414`
    (delete) — five separate spots use
    `msg.contains("already exists")`, `msg.contains("not_found")`,
    `msg.contains("not found")`, or `msg.contains("last_user_protected")`
    to pick the HTTP status + structured `code` for the response.
  - `crates/backend-backendd/src/runtime_api/invocations.rs:320-340` —
    finalize sniffs `msg.contains("schema_version")` on a
    `ResourceBlob::from_canonical_bytes` error to decide between the
    `schema-unknown` and `decode-failed` wire reasons.
  - Production-side string emission lives at
    `crates/backend-backendd/src/users.rs:122-127, 154-163, 178-179`
    (the message strings the sniffers depend on).
- **Observation:** The handler decides the HTTP status code from the
  rendered error message — a brittle coupling. If anyone reflows
  `"last_user_protected"` to `"the last user cannot be deleted"`, the
  500 → 409 mapping silently flips back to 500. The runtime case is
  worse because the matched string lives in `shared-store`'s
  `ResourceBlob::from_canonical_bytes` and is not a stable interface.
- **Recommendation:** Introduce a typed `UserMutationError` (or similar)
  enum returned by `UserCollection::{add, update, remove}` with
  variants like `NotFound`, `DuplicateUsername`, `LastUserProtected`,
  `InvalidUsername`, `BadHash`. The four user handlers then `match`
  on the variant. For the blob path, expose a typed error from
  `ResourceBlob::from_canonical_bytes` in `shared-store` (separate
  ticket — see Out-of-scope below) so the finalize handler can switch
  on `BlobParseError::UnsupportedSchema { .. }` vs decode failures.
- **Behavior change?** no for users.rs (the response codes/messages
  stay identical); finalize's `reason` strings stay identical because
  they're picked by the new variant. The runtime-side fix needs
  protocol-crate touches and gets its own ticket.
- **Related tickets:** none.

### F-006 — `_endpoint`/inner-handler split is dead boilerplate after the test surface narrowed

- **Category:** 6 (Complexity) + 7a (Simplification)
- **Severity:** must-fix
- **Scope:** in-component
- **Location(s):** every admin module has the pair —
  `handle_*_endpoint(state, ..., request)` calls `parse_json_body`
  then forwards to `handle_*(state, ..., &body)`. Sites:
  - `pairs.rs:40-79` (3 pairs).
  - `policy.rs:121-167` (3 pairs).
  - `resources.rs:40-126` (6 pairs).
  - `client_instances.rs:173-183, 296-307` (2 pairs).
  - `secrets.rs:101-115, 193-206` (2 pairs).
  - `users.rs:117-127, 209-220` (2 pairs).
  - `stores/crud.rs:90-100, 251-262` (2 pairs).
  - `stores/lifecycle.rs:368-383` (1 pair).
- **Observation:** The split exists so a unit test can drive the inner
  function with a typed body without synthesising a `Request<Incoming>`
  (which `hyper` doesn't expose constructors for). But ~half of these
  inner functions have **no remaining direct test caller** —
  e.g. `handle_admin_pair_create` is only called by its `_endpoint`
  twin and from no `tests.rs` (`rg -n
  'handle_admin_pair_create\b' crates/backend-backendd/src/**/tests.rs`
  returns nothing). Where a test *does* call the inner form
  (`stores/crud.rs:106` notes "Split out so unit tests can drive it
  without synthesising a `Request<Incoming>`") the split is justified;
  elsewhere it's pure ceremony.
- **Recommendation:** Audit each pair. Where no test calls the
  body-already-parsed form, inline `parse_json_body` into the single
  `handle_admin_*` function and delete the `_endpoint` wrapper. Where a
  test does, keep the split but rename `_endpoint` →
  `handle_admin_*` and the inner one to `handle_admin_*_with_body`
  (or fold the test to use a real bytes round-trip).
- **Behavior change?** no.
- **Related tickets:** none.

### F-007 — Cargo.toml description is stale and refers to "Unix socket (for SSH tunnel)"

- **Category:** 2 (Naming & terminology) + 10 (Doc & comment hygiene)
- **Severity:** must-fix
- **Scope:** in-component
- **Location(s):** `crates/backend-backendd/Cargo.toml:3` —
  `description = "underwrap backend daemon — holds secrets, serves them over Unix socket (for SSH tunnel)"`.
- **Observation:** The backend has been an HTTPS+mTLS service for a
  while now; the SSH-forwarded Unix socket transport is gone (see
  `docs/backend-release-plan.md:31`). The crate description is a
  pre-cutover relic.
- **Recommendation:** Replace with something like `"underwrap backend
  daemon — HTTPS service that holds secrets, serves the runtime mTLS
  API, and hosts the admin SPA + JSON API"`.
- **Behavior change?** no.
- **Related tickets:** none.

### F-008 — `tempfile` is in both `[dependencies]` and `[dev-dependencies]`

- **Category:** 4 (Dead code & API hygiene)
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `crates/backend-backendd/Cargo.toml:48, 51`.
- **Observation:** A regular dep already pulls `tempfile` in for
  every build (it's used by atomic-write helpers in
  `store_registry.rs`, `runtime_clients.rs`, and `users.rs`); the
  dev-dep entry is redundant.
- **Recommendation:** Drop the `[dev-dependencies]` line; the regular
  dep keeps tests building.
- **Behavior change?** no.
- **Related tickets:** none.

### F-009 — Hand-rolled hex encoding in four places, `hex` crate already a dep

- **Category:** 7 (Duplication / missed reuse) + 7a (Simplification)
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):**
  - `crates/backend-backendd/src/runtime_clients.rs:71-74` —
    `cert_fingerprint_from_der` rolls its own hex.
  - `crates/backend-backendd/src/ids.rs:14-22` — `random_id`'s inner loop.
  - `crates/backend-backendd/src/ids.rs:39-48` — `random_hex_8`.
  - `crates/backend-backendd/src/admin_api/stores/mod.rs:240-242` —
    `master_key_hex`.
- **Observation:** All four functions hand-write `format!("{b:02x}")`
  loops; `hex::encode` produces the same lowercase-no-prefix output and
  the crate is already in `Cargo.toml:38`. (Confirmed:
  `crates/backend-backendd/src/users.rs:227-231` already uses
  `hex::encode` for `new_user_id`.)
- **Recommendation:** Collapse all four to `hex::encode(&bytes)`.
- **Behavior change?** no (`hex::encode` produces identical strings).
- **Related tickets:** none.

### F-010 — `is_valid_store_id` and `is_valid_client_instance_id` are byte-for-byte duplicates

- **Category:** 7 (Duplication / missed reuse)
- **Severity:** must-fix
- **Scope:** in-component
- **Location(s):**
  - `crates/backend-backendd/src/store_registry.rs:260-272` —
    `is_valid_store_id`.
  - `crates/backend-backendd/src/runtime_clients.rs:236-248` —
    `is_valid_client_instance_id`.
- **Observation:** Both implement the same rule ("lowercase letters,
  digits, `-`, `_`, starting with an alphanumeric") with literally the
  same code. The only difference is which crate-local function name
  callers reach for.
- **Recommendation:** Move one canonical `is_valid_simple_id(name: &str) -> bool`
  into `crate::ids` (already the home for id helpers) and call it from
  both validators. Keep the public re-exports
  (`is_valid_store_id` is re-exported via `lib.rs:28-31`) as one-line
  wrappers if external callers depend on the shape, or rename them
  away if not (the `integration-tests` and `ts_export` consumers only
  reach for the types listed in `lib.rs:21-31` — verify they don't
  touch the validators directly).
- **Behavior change?** no.
- **Related tickets:** none.

### F-011 — Repeated `SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(0).as_secs()` pattern; `users::now_epoch_secs()` already exists

- **Category:** 7 (Duplication / missed reuse) + 7a (Simplification)
- **Severity:** must-fix
- **Scope:** in-component
- **Location(s):**
  - `crates/backend-backendd/src/admin_api/auth.rs:110-113, 167-170,
    207-210` — login, logout, whoami.
  - `crates/backend-backendd/src/admin_api/csrf.rs:58-61` —
    `authenticate_admin`.
  - `crates/backend-backendd/src/admin_api/overview.rs:65-68` —
    `handle_admin_ui_overview` (and the value is then unused, see
    F-003).
  - `crates/backend-backendd/src/test_support.rs:261-264` — session
    builder.
  - Production helper: `crates/backend-backendd/src/users.rs:235-240`,
    `now_epoch_secs()`.
- **Observation:** `users::now_epoch_secs()` is the canonical helper
  and is used in 11 places already. The six call sites above
  re-implement the same `SystemTime::now()` dance inline.
- **Recommendation:** Replace each with `crate::users::now_epoch_secs()`.
- **Behavior change?** no.
- **Related tickets:** none.

### F-012 — Session-cookie issuing duplicated across login + bootstrap

- **Category:** 7 (Duplication / missed reuse)
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):**
  - `crates/backend-backendd/src/admin_api/auth.rs:110-156` — login
    response build (payload → encrypt → render two cookies → JSON body
    → `Response::builder()` with two `SET_COOKIE` headers).
  - `crates/backend-backendd/src/admin_api/bootstrap.rs:186-225` —
    same dance with a different success JSON body and log message.
- **Observation:** ~30 lines of identical session-issuing logic.
- **Recommendation:** Extract `issue_session_response<B: Serialize>(state,
  payload, body) -> Response<Full<Bytes>>` that handles encrypt +
  render two cookies + build the response. Login and bootstrap each
  collapse to a ~5-line call.
- **Behavior change?** no.
- **Related tickets:** none.

### F-013 — Runtime open + finalize duplicate the policy-lookup → deny-response translation

- **Category:** 6 (Complexity) + 7 (Duplication)
- **Severity:** must-fix
- **Scope:** in-component
- **Location(s):**
  - `crates/backend-backendd/src/runtime_api/invocations.rs:130-183`
    (open) and `:246-299` (finalize) — both load the policy document,
    call `resolve_invocation`, then match all four variants of
    `InvocationLookup` to produce either `(resource_id, writable)` or
    a `json_response(Deny{reason})`. The four match arms are
    structurally identical; only the response type differs
    (`RuntimeWrapperInvocationOpenResponse::Deny` vs
    `RuntimeWrapperInvocationFinalizeResponse::Deny`).
- **Observation:** The two response enums share the `Deny { reason }`
  shape but are distinct types. The duplication is forced by the
  type-distinct deny payloads.
- **Recommendation:** Extract a helper
  `resolve_invocation_or_deny<F>(store, principal, body, into_deny:
  F) -> Result<(String, bool), Response<Full<Bytes>>>` where `F` maps
  a `String` reason to the per-handler deny response. Both handlers
  collapse the 50-line block to one call. The internal-error fallthrough
  on `load_policy_document`/`resolve_invocation` becomes shared too.
- **Behavior change?** no.
- **Related tickets:** none.

### F-014 — `whoami` silently treats a corrupt user collection as "no users" via `unwrap_or_default()`

- **Category:** 3 (Error handling)
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `crates/backend-backendd/src/admin_api/auth.rs:225` —
  `let collection = crate::users::load(&state.users_file).unwrap_or_default();`
- **Observation:** Every other admin endpoint that reads the user
  collection treats a load failure as 500. `whoami` swallows it and
  returns `authenticated: false`. That keeps the redirect dance
  working but masks a real disk problem from the operator.
- **Recommendation:** Either log-and-return-500 like the other
  handlers, or keep the current behaviour but at least emit a
  `warn!(err = %e, "whoami: user collection unreadable")` so the
  operator sees the underlying error in the logs.
- **Behavior change?** yes (success → 500 in the load-failure path)
  if we route to 500. If we keep behaviour and only add the warn, no.
  Recommend the warn-only path; an admin SPA stuck on the login page
  is a worse UX than a `500` in devtools.
- **Related tickets:** none.

### F-015 — `runtime_clients.rs` lacks a top-of-file `//!` doc comment

- **Category:** 10 (Doc & comment hygiene)
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `crates/backend-backendd/src/runtime_clients.rs:1`
  — file opens straight on `use anyhow::{...};`. Every other module
  in the crate (`auth.rs`, `tls.rs`, `web.rs`, `users.rs`,
  `store_registry.rs`, `runtime_state.rs`, `config.rs`, `ids.rs`,
  `admin_api/*`, `runtime_api/*`, `http/*`, `storage/*`) opens with a
  `//!` summary.
- **Recommendation:** Add a `//!` paragraph summarising what
  `ClientInstanceRecord` / `CurrentCredentialRecord` /
  `ClientInstancesDocument` are and how they relate to the runtime
  mTLS handshake.
- **Behavior change?** no.
- **Related tickets:** none.

### F-016 — `runtime_state.rs` carries a `// see epic_backend_modularization` pointer

- **Category:** 9 (Greenfield cleanups)
- **Severity:** must-fix
- **Scope:** in-component
- **Location(s):** `crates/backend-backendd/src/runtime_state.rs:12-13`
  — `//! Splitting this struct into smaller per-concern structs is
  deferred — see `epic_backend_modularization`.`
- **Observation:** The skill posture explicitly forbids ticket / epic
  pointers inside source files. The epic in question is also closed
  (ticket 1220 `Final simplification sweep + close admin_api refinement
  epic` is in `tickets/03_done/`).
- **Recommendation:** Drop the sentence (or, if the
  split-into-per-concern idea is still worth keeping in the code as a
  design note, rewrite it without naming the epic — just say "Future
  refinement could split this struct into per-concern bundles; not
  worth doing while the field count is this small").
- **Behavior change?** no.
- **Related tickets:** none.

### F-017 — Legacy-config rejection in `main.rs` is dead weight pre-first-release

- **Category:** 9 (Greenfield cleanups)
- **Severity:** must-fix
- **Scope:** in-component
- **Location(s):** `crates/backend-backendd/src/main.rs:53-69` —
  `parse_backend_config` re-parses the YAML once just to detect a
  `stores:` key and bail with a migration hint; `:193-208` is the
  matching test
  (`parse_backend_config_rejects_legacy_inline_stores_with_migration_hint`).
- **Observation:** There are no deployed users; the inline
  `stores:` shape was never shipped to anyone. The "give the operator a
  clear migration hint" justification only applies if there is an
  operator with the old shape on disk. There isn't.
- **Recommendation:** Delete the `if let serde_yaml::Value::Mapping(map)`
  pre-check, fold `parse_backend_config` back to a single
  `serde_yaml::from_str` call, and delete the test. `serde_yaml` will
  return "unknown field `stores`" on its own, which is sufficient
  greenfield-error-handling.
- **Behavior change?** marginal — the error message changes from a
  curated bail() to serde's default. Acceptable per the posture.
- **Related tickets:** none.

### F-018 — Inline `mod tests { ... }` block in `main.rs` (in scope) plus same pattern across the workspace (project-wide)

- **Category:** 5 (Test layout & coverage) + 9 (Greenfield cleanups)
- **Severity:** must-fix
- **Scope:** in-component for `main.rs`; **project-wide** for the rest.
- **Location(s):**
  - In scope: `crates/backend-backendd/src/main.rs:169-286` — 118-line
    inline test block.
  - Project-wide (each well over the ~30-line tiny exception):
    `crates/shared-protocol/src/resource_blob.rs:133` (256 lines),
    `crates/shared-protocol/src/entities.rs:248` (239),
    `crates/shared-protocol/src/config.rs:198` (185),
    `crates/frontend-keygen/src/lib.rs:291` (152),
    `crates/shared-store/src/path.rs:160` (128),
    `crates/shared-protocol/src/policy.rs:176` (102),
    `crates/shared-protocol/src/ipc.rs:322` (100),
    `crates/shared-protocol/src/exec.rs:83` (92),
    `crates/shared-store/src/encrypted.rs:114` (85),
    `crates/shared-protocol/src/resource.rs:213` (82),
    `crates/frontend-underwrapd/src/audit.rs:135` (80),
    `crates/frontend-secretsd/src/dbus_service/service.rs:224` (72),
    `crates/frontend-secretsd/src/main.rs:262` (68),
    `crates/frontend-secretsd/src/dbus_service/item.rs:177` (66),
    `crates/frontend-secretsd/src/section_store.rs:122` (65),
    `crates/frontend-secretsd/src/state.rs:82` (64),
    `crates/frontend-ns/src/env.rs:70` (63),
    `crates/shared-store/src/lib.rs:130` (61),
    `crates/shared-protocol/src/message/wrapper_resource.rs:143` (60),
    `crates/shared-protocol/src/scm_rights.rs:119` (50),
    `crates/frontend-underwrapd/src/server.rs:252` (46),
    `crates/frontend-underwrapd/src/backend_client.rs:322` (45),
    `crates/frontend-ns/src/libsecret.rs:196` (45),
    `crates/frontend-keyring-probe/src/main.rs:249` (41),
    `crates/frontend-underwrapd/src/startup.rs:170` (38),
    `crates/integration-tests/src/harness/builders.rs:55` (37),
    `crates/frontend-ns/src/directory_credential.rs:303` (35),
    `crates/frontend-secretsd/src/dbus_service/collection.rs:179` (35).
  - Sub-threshold (tiny exception applies, leave alone):
    `frontend-underwrapd/src/main.rs:59` (28 lines),
    `shared-protocol/src/message.rs:19` (26),
    `frontend-exec/src/main.rs:463` (21),
    `frontend-secretsd/src/dbus_service.rs:100` (18),
    `frontend-underwrapd/src/storage.rs:5` (16),
    `frontend-underwrapd/src/lib.rs:11` (11),
    `shared-protocol/src/service.rs:850` (17).
- **Observation:** `AGENTS.md` mandates the sibling-file `tests.rs`
  pattern. Ticket 1210 in `03_done/` relocated the inline tests inside
  this crate; the rest of the workspace was not done.
- **Recommendation:** For `main.rs`: split the inline block into
  `crates/backend-backendd/src/main_tests.rs` (binaries can't use the
  sibling-submodule trick directly; instead, move `parse_backend_config`
  + `recover_user` into the library crate and unit-test them from
  `src/users/tests.rs` or `src/config/tests.rs`). For the rest: one
  repo-wide sweep ticket that relocates each over-threshold inline
  block to a sibling `tests.rs`.
- **Behavior change?** no.
- **Related tickets:** ticket 1210 (in `03_done/`, did the backend-backendd
  half).

### F-019 — Ticket / epic / story pointers scattered through `crates/`, `web/`, `nix/`, `docs/`, `examples/`

- **Category:** 9 (Greenfield cleanups)
- **Severity:** must-fix
- **Scope:** **project-wide** (one in-scope; the rest outside this
  component but still required cleanup per the posture).
- **Location(s):**
  - In scope:
    `crates/backend-backendd/src/runtime_state.rs:13` (covered by F-016),
    `crates/backend-backendd/tests/ts_export.rs:171, 191` (ticket 384,
    ticket 404).
  - Out of scope (32 total occurrences across the repo):
    `web/playwright.config.ts:38` (ticket 252),
    `web/e2e/secrets.spec.ts:2` (ticket 300),
    `web/e2e/profile.spec.ts:2` (ticket 294),
    `web/e2e/auth-flows.spec.ts:4` (ticket 224),
    `web/e2e/backendd.ts:73` (ticket 234),
    `web/e2e/dashboard.spec.ts:2` (ticket 294),
    `web/e2e/users.spec.ts:2` (ticket 293),
    `web/e2e/stores.spec.ts:2, 73, 249` (ticket 233, 234, 240),
    `web/e2e/multi-user.spec.ts:2` (ticket 243),
    `web/src/api/client.ts:293, 412, 501, 557` (tickets 231, 240, 300, 384),
    `nix/tests/node.nix:271` (ticket 262),
    `nix/tests/test.py:111, 369, 1139, 1172, 1921` (tickets 212, 231,
    432, story 410, `epic_runtime_api_redesign`),
    `docs/backend.md:62` (ticket 230),
    `crates/shared-store/src/resource_blob/tests.rs:102, 127` (ticket 405),
    `crates/integration-tests/src/harness/readiness.rs:75, 90` (tickets 280, 281),
    `crates/integration-tests/src/tests/wrapper_writeback.rs:467` (ticket 423).
- **Observation:** Repo-wide rule per `CLAUDE.md` "Posture" section:
  "tickets and commit messages are the right place for that context;
  the code is not."
- **Recommendation:** One sweep ticket that strips every ticket-number /
  epic_*/story_* / `TODO(<n>)` / `FIXME(<n>)` reference. Where a
  comment is load-bearing (e.g. nix/tests/test.py:111 "the backend's
  CSRF double-submit check (ticket 212) passes" — the part about the
  CSRF check matters), keep the description and drop only the parenthetical.
- **Behavior change?** no.
- **Related tickets:** none.

### F-020 — `auth.rs` rustdoc still references "Next.js BFF"

- **Category:** 2 (Naming & terminology) + 9 (Greenfield cleanups)
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):**
  - `crates/backend-backendd/src/auth.rs:7` —
    "the 32-byte session key file (already used by the Next.js BFF)
    is repurposed as a ChaCha20-Poly1305 AEAD key."
  - `crates/backend-backendd/src/auth.rs:30-32` — "the Next.js BFF
    used the same length."
- **Observation:** The Next.js BFF is gone; the SPA now talks directly
  to the backend HTTPS listener. The rustdoc is the only remaining
  trace.
- **Recommendation:** Drop the BFF parenthetical. Say "The session
  key file holds 32 random bytes; this module repurposes it as a
  ChaCha20-Poly1305 AEAD key …" without the historical aside.
- **Behavior change?** no.
- **Related tickets:** none.

## Out-of-scope / routed elsewhere

- **OS-1 (related to F-005, runtime path)**: introducing a typed
  `BlobParseError` in `shared-store` so `runtime_api/invocations.rs`
  can stop sniffing for `"schema_version"` in the error string
  requires touching `crates/shared-store/src/resource_blob.rs` (out
  of this component's scope). Route as a separate non-refactor task
  ticket owned by the user. The user-CRUD half of F-005 stays
  in-scope because it lives entirely inside `backend-backendd`.

- **OS-2 (general comment style)**: the WHAT-comments rule (skill
  posture, Doc & comment hygiene category) appears mostly respected
  in this crate — most comments explain *why* (audit shape,
  fingerprint-vs-chain distinction in TLS, etc.). I noted no
  systematic violation worth a sweep ticket inside the component
  scope. Owner: user (no action recommended).

## Ticket plan

Epic shape selected because there are 8 in-scope tasks and 2 repo-wide
sweep tasks, several non-trivial (F-001 schema-version removal touches
both backend-backendd and shared-protocol; F-005 user-CRUD error typing
touches `users.rs`, every user handler, and the test asserts; F-006
endpoint-collapse touches every admin module).

New tickets pick the `1300_` block (next free above the recently-done
`1230_` ticket).

Numbering picks ordering so later tasks build on earlier ones — the
schema-version removal lands first because it changes serde shapes
that other refactors otherwise have to re-touch; the typed-error
rewrites and `_endpoint` collapse come next; the repo-wide sweeps
land last so they apply to a stable post-refactor codebase.

```
tickets/01_todo/
  epic_backend_backendd_refinement_2026-05-22.md
  1300_story_backend_backendd_refinement.md
  1310_task_drop_pre_first_release_schema_versioning.md       (F-001)
  1320_task_typed_user_mutation_error.md                       (F-005, user-CRUD half)
  1330_task_drop_dead_audit_state_param.md                     (F-002)
  1340_task_drop_dead_overview_now_epoch.md                    (F-003)
  1350_task_collapse_endpoint_wrappers.md                      (F-006)
  1360_task_extract_session_issue_helper.md                    (F-012)
  1370_task_unify_invocation_lookup_helper.md                  (F-013)
  1380_task_consolidate_id_helpers_and_validators.md           (F-009, F-010, F-011)
  1390_task_misc_doc_and_dep_hygiene.md                        (F-004, F-007, F-008, F-014, F-015, F-016, F-017, F-020)
  1400_task_repo_sweep_relocate_inline_tests.md                (F-018, project-wide)
  1410_task_repo_sweep_strip_ticket_references.md              (F-019, project-wide)
```

### Per-ticket sketches

#### `epic_backend_backendd_refinement_2026-05-22.md`

> Refinement epic spun off the 2026-05-22 backend-backendd review.
> Covers schema-versioning removal, typed user-mutation errors,
> dead-parameter cleanup, endpoint/inner-handler collapse,
> duplication consolidation across id and hex helpers, and assorted
> doc/hygiene fixes inside the crate. Plus two repo-wide sweep
> tasks: relocate the remaining inline `mod tests {}` blocks across
> the workspace, and strip every ticket / epic / story pointer out
> of source/config/docs.

Children: `1300_story_backend_backendd_refinement.md` + the ten
task tickets below.

Acceptance: every child ticket is in `03_done/`, the whole-workspace
build is green (`cargo build`, `cargo test --workspace`,
`cargo clippy --all-targets --workspace -- -D warnings`,
`nix build .#checks.x86_64-linux.vm-test`).

#### `1300_story_backend_backendd_refinement.md`

Story under the epic. Restates the scope, links each task ticket,
and notes the ordering constraint (1310 lands first because it
changes serde shapes; the sweeps land last).

#### `1310_task_drop_pre_first_release_schema_versioning.md` (F-001)

> Drop the `version: u32` field, its `default_*_version` helper, its
> `serde(default)` attribute, and the matching `ensure!` from every
> persisted document this crate owns: `UserStateDocument`,
> `ClientInstancesDocument`, `StoreRegistryDocument`. Also drop the
> matching protocol-side stamps on `ResourcesDocument` and
> `PairsDocument` (touch `crates/shared-protocol/src/entities.rs`
> and the `ENTITY_SCHEMA_VERSION` constant). Drop the
> "older test fixtures" `serde(default)` on
> `CurrentCredentialRecord.issued_at` and
> `ClientInstanceRecord.auto_create_wrapper_assets`; update the
> tests / fixtures that omitted those fields. Per the posture this
> is pure dead-weight removal — no shape was ever deployed.

Acceptance: `cargo test --workspace`,
`cargo clippy --all-targets --workspace -- -D warnings`,
`nix build .#checks.x86_64-linux.vm-test`.

#### `1320_task_typed_user_mutation_error.md` (F-005, in-scope half)

> Introduce `UserMutationError` (NotFound, DuplicateUsername,
> LastUserProtected, InvalidUsername, BadHash) returned by
> `UserCollection::{add, update, remove}` (`crates/backend-backendd/src/users.rs`).
> Switch the four user handlers and the password-change handler in
> `admin_api/users.rs` + `admin_api/auth.rs` to match on the variant
> instead of `msg.contains(...)`. HTTP status codes and structured
> response `code`s stay byte-for-byte identical (verify via the
> existing tests). Leave the runtime-side `schema_version` sniffing
> alone — that needs `shared-store` changes routed to a separate
> non-refactor ticket (Out-of-scope OS-1 in the review).

Acceptance: `cargo test --workspace`,
`cargo clippy --all-targets --workspace -- -D warnings`. The existing
tests in `users/tests.rs` and `admin_api/users/tests.rs` should pass
unchanged.

#### `1330_task_drop_dead_audit_state_param.md` (F-002)

> Drop the unused `_state: &AdminRuntimeState` parameter from
> `write_admin_permission_audit`, `write_admin_user_audit`,
> `audit_bootstrap_attempt`, and `write_admin_store_audit`. Update
> every call site.

Acceptance: `cargo test --workspace`,
`cargo clippy --all-targets --workspace -- -D warnings`.

#### `1340_task_drop_dead_overview_now_epoch.md` (F-003)

> Drop the unused `_now_epoch: u64` parameter from
> `admin_ui_overview_for_store`. Drop the `now_epoch` computation
> in `handle_admin_ui_overview`.

Acceptance: `cargo test --workspace`,
`cargo clippy --all-targets --workspace -- -D warnings`.

#### `1350_task_collapse_endpoint_wrappers.md` (F-006)

> Audit every `handle_admin_*_endpoint` / `handle_admin_*` pair
> under `admin_api/`. For each pair where the inner function has no
> direct caller in any `tests.rs`, inline `parse_json_body` into one
> function and drop the wrapper. For pairs where a test does call
> the inner form (notably `handle_admin_store_create` and
> `handle_admin_store_update`), keep the split but rename so the
> body-already-parsed form has the descriptive name and the wrapper
> is the bare `handle_admin_*` HTTP entry-point.

Acceptance: `cargo test --workspace`,
`cargo clippy --all-targets --workspace -- -D warnings`. Final
simplification pass against category 7a (per the skill's "Pairing
with the execution session" note): re-read each touched admin module
and remove anything still fitting the simplification bullet list.

#### `1360_task_extract_session_issue_helper.md` (F-012)

> Extract `issue_session_response<B: Serialize>(state, payload, body)
> -> Response<Full<Bytes>>` (location: `admin_api/auth.rs` or a new
> `admin_api/sessions.rs`, your call). Replace the duplicated
> session-issuing logic in `handle_admin_login` and
> `handle_admin_bootstrap`. Mirror in `test_support::run_bootstrap`
> if its open-coded version still lingers after the helper lands.

Acceptance: `cargo test --workspace`,
`cargo clippy --all-targets --workspace -- -D warnings`.

#### `1370_task_unify_invocation_lookup_helper.md` (F-013)

> Extract `resolve_invocation_or_deny<F>(store, principal,
> wrapper_name, resource_name, into_deny: F) -> Result<(String, bool),
> Response<Full<Bytes>>>` from
> `runtime_api/invocations.rs`. Both
> `handle_runtime_wrapper_invocation_open` and
> `handle_runtime_wrapper_invocation_finalize` call it with a
> per-handler closure that turns a `String` reason into the right
> `Deny { reason }` response. The 50-line duplicated match
> collapses to one call per handler.

Acceptance: `cargo test --workspace`,
`cargo clippy --all-targets --workspace -- -D warnings`. The existing
tests in `runtime_api/invocations/tests.rs` should pass unchanged.

#### `1380_task_consolidate_id_helpers_and_validators.md` (F-009, F-010, F-011)

> 1. Move `is_valid_store_id` / `is_valid_client_instance_id`'s
>    shared body into `crate::ids` as `is_valid_simple_id`. Update
>    the two existing call paths to wrap it (or delete the wrappers
>    if no external caller relies on the name — verify against
>    `integration-tests` and the `ts_export` binding test).
> 2. Replace the four hand-rolled hex loops (`runtime_clients.rs`,
>    `ids.rs:14-22`, `ids.rs:39-48`, `stores/mod.rs:240-242`) with
>    `hex::encode(&bytes)`.
> 3. Replace the six inlined `SystemTime::now().duration_since(...).as_secs()`
>    sequences in `admin_api/auth.rs`, `admin_api/csrf.rs`,
>    `admin_api/overview.rs`, and `test_support.rs` with
>    `crate::users::now_epoch_secs()`.

Acceptance: `cargo test --workspace`,
`cargo clippy --all-targets --workspace -- -D warnings`.

#### `1390_task_misc_doc_and_dep_hygiene.md` (F-004, F-007, F-008, F-014, F-015, F-016, F-017, F-020)

> Catch-all for the small wins:
> - Drop unused `_stores: &[&str]` param from
>   `test_support::admin_principal`. Update call sites. (F-004)
> - Rewrite the stale `Cargo.toml` description. (F-007)
> - Remove the duplicate `tempfile` `[dev-dependencies]` entry. (F-008)
> - Add `warn!` logging to the `whoami` user-collection load
>   fallback. (F-014)
> - Add a `//!` module summary to `runtime_clients.rs`. (F-015)
> - Strip the `epic_backend_modularization` pointer out of
>   `runtime_state.rs`. (F-016)
> - Delete the legacy inline `stores:` rejection in
>   `parse_backend_config` and its test. (F-017)
> - Drop the "Next.js BFF" remnants from `auth.rs` rustdoc. (F-020)

Acceptance: `cargo test --workspace`,
`cargo clippy --all-targets --workspace -- -D warnings`,
`nix build .#checks.x86_64-linux.vm-test`.

#### `1400_task_repo_sweep_relocate_inline_tests.md` (F-018, project-wide)

> Repo-wide sweep ticket. Relocate every inline `#[cfg(test)] mod
> tests { ... }` block longer than ~30 lines into a sibling
> `tests.rs` per the `AGENTS.md` convention. Affects ~27 files
> outside `backend-backendd` (full list in finding F-018) plus
> `backend-backendd/src/main.rs` (which needs the
> move-into-library trick because binaries can't use the
> sibling-submodule pattern directly).
> Do **not** touch the sub-threshold inline blocks listed in the
> finding's "Sub-threshold" section — those stay.

Acceptance: `cargo test --workspace`,
`cargo clippy --all-targets --workspace -- -D warnings`. No
behaviour change, pure relocation.

#### `1410_task_repo_sweep_strip_ticket_references.md` (F-019, project-wide)

> Repo-wide sweep ticket. Strip every ticket-number / `epic_*` /
> `story_*` / `TODO(<n>)` / `FIXME(<n>)` reference from
> source/config/docs (excluding the `tickets/` tree itself). 32
> occurrences listed in finding F-019. Where a comment is
> load-bearing apart from the parenthetical (e.g. nix/tests/test.py's
> CSRF check note), keep the descriptive text and drop only the
> ticket pointer.

Acceptance: `rg -n -i '(ticket[[:space:]]*[0-9]{3,4}|epic_[a-z_]+|story_[a-z_]+|TODO\([0-9]{3,4}\)|FIXME\([0-9]{3,4}\))' crates/ web/ nix/ docs/ examples/`
returns nothing (excluding ticket paths). `cargo test --workspace`,
`cargo clippy --all-targets --workspace -- -D warnings`,
`nix build .#checks.x86_64-linux.vm-test`.

---

Approve the plan and start a fresh session — it will pick this report
up from `tickets/_review/` and create the tickets, then execute them
via `underwrap-ticket-workflow`.
