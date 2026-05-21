# Refinement review: `backend-backendd` crate

Date: 2026-05-21
Reviewer session: claude-opus-4-7[1m]
Scope: the entire `crates/backend-backendd` crate — 19 production
files (~6 900 LoC), 13 sibling `tests.rs` files (~3 800 LoC), one
build script, and the `tests/ts_export.rs` integration test. The
modularization epic in `tickets/01_todo/epic_backend_modularization.md`
is already implemented (server.rs is 207 lines, the target layout is
present), so this review judges the *post-split* shape rather than the
old monolith. Dependents read: `main.rs`, `crates/integration-tests`,
`crates/backend-backendd/tests/ts_export.rs` — every external caller
imports from `underwrap_backendd::server::*`, which is the load-bearing
reason the `pub use` shim still exists.

## Summary

- 13 findings across 7 categories.
- 6 must-fix, 7 nice-to-have, 0 out-of-scope (user resolved F-013:
  drop the v1 migration path entirely; new code only).
- Proposed ticket shape: **epic + story + tasks** (12 task tickets, ordered
  so each only depends on earlier ones). The modularization epic
  already in `01_todo/` should be moved to `03_done/` as a prerequisite
  housekeeping step, since the work it describes is finished.

## Findings

### F-001 — Stop-gap `crate::server::admin_store` re-export still in use everywhere

- **Category:** Dead code & API hygiene
- **Severity:** must-fix
- **Location:** `crates/backend-backendd/src/server.rs:38-42`,
  imported from `admin_api/{pairs,secrets,resources,policy}.rs` in 21
  places.
- **Observation:** `server.rs:38-42` re-exports the helper with a
  self-described stop-gap comment:

      // `admin_store` is re-exported because most `admin_api/*` modules
      // still import it as `crate::server::admin_store`. The function lives
      // next to its production code in `admin_api::stores`; this re-export
      // is a stop-gap until those imports get updated.
      pub(crate) use crate::admin_api::stores::admin_store;

  Every admin handler in `pairs.rs`, `secrets.rs`, `resources.rs`, and
  `policy.rs` then opens with `use crate::server::admin_store;`
  (21 occurrences). The "until then" never came.
- **Recommendation:** Replace each `use crate::server::admin_store;`
  with `use crate::admin_api::stores::admin_store;` (one short import
  at module top, not in-function). Remove the `pub(crate) use` line in
  `server.rs:42` and the leading comment.
- **Behavior change?** no.
- **Related tickets:** finishes the cleanup the modularization
  epic explicitly identified as a follow-up.

### F-002 — `server.rs` re-exports load-bearing for external consumers; the comment misleads

- **Category:** Dead code & API hygiene
- **Severity:** nice-to-have
- **Location:** `crates/backend-backendd/src/server.rs:22-36`,
  consumed by `crates/integration-tests/src/{harness/mod.rs,tests/daemon.rs}`,
  `crates/backend-backendd/tests/ts_export.rs:28-31`, `src/main.rs:6`.
- **Observation:** The `pub use` block at the top of `server.rs` says
  "Once the crate root grows its own re-exports we can drop these —
  for now `server` remains the import path of record." External
  callers reach `BackendConfig`, `HttpsConfig`, `HttpsTlsConfig`,
  `AdminConfig`, `BootstrapRequest`, `LoginRequest` etc. through
  `underwrap_backendd::server::*`. The block isn't dead — it's the
  crate's public API surface, and the modules it re-exports from
  (`config`, `store_registry`, `admin_api::auth`, `admin_api::bootstrap`)
  are all `pub(crate)` so they aren't reachable any other way.
- **Recommendation:** Decide which way the public API points. Two
  options, both safe:
  1. Lift the re-exports to `lib.rs`, drop the comment, leave
     `server.rs` to expose only `run()`. External callers then
     import from `underwrap_backendd::*` instead of `…::server::*`.
     This is what the comment hints at and matches Rust conventions.
  2. Keep `server.rs` as the import path of record, but delete the
     "until then" comment so nobody confuses it with the F-001
     stop-gap.
- **Behavior change?** no (option 1 changes import paths in three
  external files; option 2 only deletes a comment).
- **Related tickets:** none.

### F-003 — Dead `installed_key` placeholder in rotate-key

- **Category:** Simplification (same behaviour)
- **Severity:** must-fix
- **Location:** `crates/backend-backendd/src/admin_api/stores.rs:862-863`.
- **Observation:**

      let installed_key = generate_master_key_bytes(); // placeholder, immediately overwritten
      let _ = installed_key; // keep clippy happy if it ever sees an unused let

  Generates 32 fresh random bytes from the OS RNG that are immediately
  discarded. The two-line dance is a relic; the surrounding code never
  reads `installed_key`. Burns entropy and reads as a footgun.
- **Recommendation:** Delete both lines. The comments say what
  happens; that what doesn't actually happen.
- **Behavior change?** no.

### F-004 — `stage_name` random suffix uses `master_key_hex(&generate_master_key_bytes())`

- **Category:** Simplification (same behaviour)
- **Severity:** nice-to-have
- **Location:** `crates/backend-backendd/src/admin_api/stores.rs:778-785`.
- **Observation:**

      let stage_name = format!(
          ".rotating-{}-{}",
          name,
          master_key_hex(&generate_master_key_bytes())
              .chars()
              .take(16)
              .collect::<String>()
      );

  Allocates a 32-byte hex string from a master-key-strength RNG only
  to lop off all but 16 chars. The `master_key_*` helpers' names mean
  something specific (32-byte master key); using them as "random
  string source" muddles that.
- **Recommendation:** Add a small `random_hex(byte_count: usize) -> String`
  helper next to `generate_master_key_bytes`, or inline an 8-byte
  `rand::thread_rng().fill_bytes(...)` and hex-encode it directly.
- **Behavior change?** no.

### F-005 — `load_policy_document` and `load_policy_document_or_default` are the same function

- **Category:** Duplication / missed reuse
- **Severity:** must-fix
- **Location:** `crates/backend-backendd/src/storage/documents.rs:26-33`
  and `:99-106`.
- **Observation:** Two helpers in the same file:

      pub(crate) fn load_policy_document(store: &dyn StorageBackend) -> Result<PolicyDocument> {
          match store.read(POLICY_DOCUMENT_SECRET_ID)? {
              Some(material) => serde_json::from_slice(material.as_ref())
                  .context("parsing backend policy document"),
              None => Ok(PolicyDocument::default()),
          }
      }
      …
      pub(crate) fn load_policy_document_or_default(
          store: &dyn StorageBackend,
      ) -> Result<PolicyDocument> {
          match store.read(POLICY_DOCUMENT_SECRET_ID)? {
              Some(material) => Ok(serde_json::from_slice(material.as_ref())?),
              None => Ok(PolicyDocument::default()),
          }
      }

  Both return `PolicyDocument::default()` when the document is absent.
  The only difference is the `.context(…)` annotation on the JSON
  parse error. The `_or_default` name suggests something semantically
  different ("default" instead of error) but the original *also*
  defaults on `None`. Callers are split arbitrarily — `policy.rs`
  uses one variant, `resources.rs::handle_admin_resource_delete` uses
  the other, and `overview.rs` uses the first.
- **Recommendation:** Keep one. Pick the `.context(…)` variant (better
  diagnostics) and drop the other. While here, `policy.rs::handle_admin_policy_get`
  reinvents the same logic inline with `store.read(...).and_then(...)`
  — switch it to call the surviving helper.
- **Behavior change?** no (error messages on malformed JSON improve).

### F-006 — Three near-identical random-id generators

- **Category:** Duplication / missed reuse
- **Severity:** nice-to-have
- **Location:**
  - `crates/backend-backendd/src/store_registry.rs:283-291` (`generate_store_id` → `store_*`)
  - `crates/backend-backendd/src/admin_api/client_instances.rs:28-36` (`generate_client_instance_id` → `inst_*`)
  - `crates/backend-backendd/src/runtime_api/wrappers.rs:278-286` (`generate_resource_id` → `res_*`)
- **Observation:** All three are byte-for-byte the same logic with
  different prefixes and byte counts (10 bytes each). The
  `runtime_api/wrappers.rs` rustdoc explicitly says "Mirrors
  `generate_client_instance_id` in shape so the two id namespaces
  look related but distinct" — the relationship is mechanical and
  belongs in one helper.
- **Recommendation:** Add a single `pub(crate) fn random_id(prefix: &str) -> String`
  somewhere central (e.g. a new `crate::ids` module, or piggy-back
  on `users.rs` next to `new_user_id`). Each caller becomes a thin
  named wrapper, or just inlines the call: `random_id("inst")`.
- **Behavior change?** no.

### F-007 — `auth.rs` over the 600-line ceiling with inline `mod tests`

- **Category:** Test layout & coverage
- **Severity:** must-fix
- **Location:** `crates/backend-backendd/src/auth.rs` (700 lines,
  inline `mod tests` from line 407 to 700 — ~293 lines of tests).
- **Observation:** `CLAUDE.md` mandates the sibling-file pattern when
  tests are longer than the production code or when a single tests
  file gets over ~600 lines. The auth tests pass that bar both ways:
  tests are ~70 % of production, and the inline block alone is 293
  lines. Other files in the same crate already follow the pattern
  (csrf, bootstrap, stores, …).
- **Recommendation:** Move the `mod tests { … }` body out to
  `src/auth/tests.rs`, replace the inline block with
  `#[cfg(test)] mod tests;`, leave production code untouched. Same
  test set, same coverage.
- **Behavior change?** no.

### F-008 — `users.rs` and `runtime_clients.rs` and `web.rs` carry oversized inline tests

- **Category:** Test layout & coverage
- **Severity:** nice-to-have
- **Location:**
  - `crates/backend-backendd/src/users.rs` (605 lines, inline tests ~209 lines)
  - `crates/backend-backendd/src/runtime_clients.rs` (383 lines, inline tests ~130 lines)
  - `crates/backend-backendd/src/web.rs` (466 lines, inline tests ~250 lines)
- **Observation:** Each is borderline by the "tests longer than
  production" rule — `web.rs` has more lines of tests than production
  code. `runtime_clients.rs` and `users.rs` are smaller but still
  meaningfully bigger than the other modules that already split out.
  The crate is inconsistent: most modules use sibling-file tests, a
  few still don't.
- **Recommendation:** Split all three to the sibling pattern in one
  ticket so the crate becomes uniform. This is a judgement call (the
  files aren't over 600 lines), so flag as nice-to-have, not must-fix.
- **Behavior change?** no.

### F-009 — Stale ticket-number comments and "until then" references

- **Category:** Doc & comment hygiene
- **Severity:** must-fix
- **Location:**
  - `src/auth.rs:16-19` — "Ticket 212 builds on this for CSRF + logout;
    ticket 213 adds role middleware; ticket 214 adds bootstrap. **The
    bearer-token path in `server::authenticate_admin` continues to
    work in parallel during the transition.**" Tickets 212/213/214
    are in `03_done/`, and there is no bearer-token path — only
    session cookies.
  - `src/admin_api/auth.rs:248-249` — "These two helpers will move to
    `admin_api::users` in ticket 770; until then they live in
    `crate::server` and the auth module reaches back for them." Both
    helpers (`mutate_collection`, `write_admin_user_audit`) are
    already in `admin_api::users`; the import a few lines down proves
    it.
  - `src/web.rs:205` — "Build `web/` (ticket 220) …" Done.
  - `src/config.rs:27`, `src/runtime_state.rs:66,74,79`,
    `src/auth.rs:72` — older `(ticket 212/214/240)` annotations on
    field rustdocs. These are footnotes that don't read as
    actionable, but the same scan should sweep them.
- **Recommendation:** Remove the "until then" lines outright (the
  state they describe has flipped). For the older `(ticket N)`
  footnotes on field rustdocs, replace with the *reason* (e.g.
  "Server-side set of revoked session ids — populated on logout so a
  stolen-but-unexpired cookie still rejects on the next request"),
  which is what `CLAUDE.md` asks for ("why, not when").
- **Behavior change?** no.

### F-010 — `validate_store_name` reused as a generic display-name validator

- **Category:** Naming & terminology
- **Severity:** nice-to-have
- **Location:** `crates/backend-backendd/src/store_registry.rs:274-281`,
  called from `admin_api/client_instances.rs:193,329` as
  `validate_display_name`-style validator (trimmed + non-empty).
- **Observation:** The helper does nothing store-specific; it's
  "trim and reject empty". The client-instance handler calls it on a
  display name and labels the error `invalid_display_name`. Reading
  the call site is confusing — the function name implies stronger
  validation than it performs.
- **Recommendation:** Rename to `validate_nonempty_display` (or
  similar) and move it to a small `text` helper module, or inline
  the three-line check at the two call sites that need it. Either
  way, drop the misleading "store_name" name.
- **Behavior change?** no.

### F-011 — `test_support.rs` carries 4–5 near-duplicate `AdminRuntimeState` builders

- **Category:** Duplication / missed reuse
- **Severity:** nice-to-have
- **Location:** `crates/backend-backendd/src/test_support.rs`,
  builders `test_admin_state`, `seeded_admin_state`,
  `service_state_with_runtime_client`, `fresh_bootstrap_state`,
  `crud_state` (lines ~163 through ~516).
- **Observation:** Each builder repeats every field of
  `AdminRuntimeState` with small per-builder variation (e.g. one
  pre-seeds a user, one wires up a runtime client, one configures
  `stores_data_root`). The struct literal appears five times. When a
  new field is added (and several have been over the last few
  tickets) every builder must be touched.
- **Recommendation:** Add one `default_admin_runtime_state(opts)` (or
  a small builder struct) that returns the canonical defaults, then
  have each test helper call it and mutate the two or three fields
  it actually cares about. Net delta is a smaller `test_support.rs`
  and one less footgun on future struct changes.
- **Behavior change?** no.

### F-012 — `http/mod.rs` TODO referring to wiring that never moved

- **Category:** Greenfield cleanups
- **Severity:** nice-to-have
- **Location:** `crates/backend-backendd/src/http/mod.rs:11-13`.
- **Observation:** The module rustdoc says:

      //! The transport-level wiring (TLS accept loop, admin-vs-service
      //! request split) currently still lives in `server.rs`; it will move
      //! here in a later modularization step.

  The modularization epic also targeted this move
  (`epic_backend_modularization.md` lines 63-68 lay out an `http/mod.rs`
  that owns `https_accept_loop`, `handle_https_connection`, and
  `handle_https_request`). The move never happened; those three
  functions still live in `server.rs:142-207`.
- **Recommendation:** Two ways forward:
  1. Move `https_accept_loop`, `handle_https_connection`,
     `handle_https_request` out of `server.rs` into `http/mod.rs`,
     leaving `run()` plus the `pub use` block in `server.rs`. This
     is what the epic prescribes.
  2. If you decide that `server.rs` is the right home for those
     after all (small, sit next to `run()` and the state
     construction), drop the TODO sentence from `http/mod.rs`.
  Either reduces drift between docs and tree.
- **Behavior change?** no.

### F-013 — `users.rs` v1→v2 migration code

- **Category:** Greenfield cleanups
- **Severity:** must-fix (user decision 2026-05-21: drop the v1 path; new code only)
- **Location:** `crates/backend-backendd/src/users.rs:200-363`
  (`UserStateDocument`, `parse_v1_document`, `STATE_FILE_VERSION`).
- **Observation:** The loader still accepts the v1 layout
  (`operators:` key plus per-record `role` and `granted_stores`) and
  silently migrates to v2 on re-save. `CLAUDE.md` says the project
  is greenfield and "backwards compatibility is not a constraint
  unless the user asks for it" — but real on-disk files in deployed
  test environments could still be v1.
- **Recommendation:** **Decision needed from the user.** If the
  greenfield posture trumps existing v1 files, the v1 branch can go
  (-~50 LoC + corresponding test). If real v1 files exist anywhere
  this code might run, leave it alone. **Do not auto-ticket either
  way.** Flag for owner: user.
- **Behavior change?** yes (drop-v1 path) — that's why this is
  out-of-scope for the refactor pass.

## Out-of-scope / routed elsewhere

- **F-013** — drop v1 user-state migration. Owner: user. Surface as a
  separate discussion ticket if you want to act on it; not packaged
  into this refactor epic.
- **`AdminRuntimeState` field-shape split.** The modularization epic
  explicitly defers this. The deferral still holds — the current
  builders (F-011) are a side-effect, not a reason to revisit. Owner:
  user.
- **Audit log schema unification.** `admin_api/audit.rs` emits
  `info!(action=…, operator=…, …)` with seven positional concepts,
  while `secrets.rs` and a few other places log
  `audit = "secret_stored"` ad-hoc. Worth a separate cross-component
  RFC, not a refactor ticket here. Owner: user.

## Ticket plan

Use **epic + story + tasks** (12 task tickets).

Pick a free 4-digit block. The existing modularization epic uses
`700_`; this review's work is the natural follow-up, so use the
`1000_` block.

### Housekeeping prerequisite (not part of the new epic)

The modularization epic in `01_todo/` is done in the tree (server.rs
is 207 lines, the layout matches the epic's target). The new session
should start by moving:

- `tickets/01_todo/epic_backend_modularization.md` → `tickets/03_done/`
- `tickets/01_todo/700_story_backend_modularization.md` → `tickets/03_done/`

This is not a real "task" — just keeping the board honest. If the
operator has reason to keep them in `01_todo/` (residual VM test, for
instance), surface that during execution.

### New tickets

```
epic_backend_backendd_refinement.md
1000_story_backend_backendd_refinement.md
  1010_task_drop_server_admin_store_shim.md          (F-001)
  1020_task_collapse_load_policy_document_helpers.md (F-005)
  1030_task_remove_rotate_key_dead_code.md           (F-003, F-004)
  1040_task_extract_random_id_helper.md              (F-006)
  1050_task_clean_stale_ticket_comments.md           (F-009)
  1060_task_rename_validate_store_name.md            (F-010)
  1070_task_split_auth_tests_sibling.md              (F-007)
  1080_task_split_users_runtime_clients_web_tests.md (F-008)
  1090_task_consolidate_admin_state_test_builders.md (F-011)
  1100_task_resolve_http_mod_todo.md                 (F-012)
  1110_task_decide_server_public_api_surface.md      (F-002)
  1120_task_final_simplification_pass.md             (sweep)
```

Order rationale:
- 1010 first because it touches 21 import sites and unblocks every
  later move.
- 1020 / 1030 / 1040 / 1050 / 1060 are independent leaf fixes — pick
  any order, but doing them before the test splits keeps each test
  rebase trivial.
- 1070 then 1080 — splits a single file each; mechanical move only.
- 1090 — touches `test_support.rs`; safest *after* the per-module
  test splits so the test set is stable.
- 1100 then 1110 — both require a small decision; bundle them at the
  end so the user can resolve both in one pass.
- 1120 is the simplification sweep called for by the skill (re-read
  touched files against the 7a checklist after the mechanical work
  lands).

### Ticket detail

Each ticket is a one-PR change with the standard acceptance gates
unless otherwise noted. The default gates are:

- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cargo test --workspace`

The VM test (`nix build .#checks.x86_64-linux.vm-test`) only needs to
re-run on the last ticket; the rest are confined to the crate and the
unit test suite catches any breakage.

---

**1010 — Drop the `crate::server::admin_store` re-export shim.**
Replace every `use crate::server::admin_store;` (21 in `pairs.rs`,
`secrets.rs`, `resources.rs`, `policy.rs`) with
`use crate::admin_api::stores::admin_store;` at module top. Remove
the `pub(crate) use` line and its comment in `server.rs:38-42`.
Resolves: F-001. Default gates.

**1020 — Collapse `load_policy_document_or_default` into
`load_policy_document`.** Keep the `.context(…)` annotation; remove
the `_or_default` variant. Update every call site
(`policy.rs`, `resources.rs`, `pairs.rs`, `overview.rs`, `wrappers.rs`,
`invocations.rs`). Also replace the inline
`store.read(POLICY_DOCUMENT_SECRET_ID).and_then(…)` in
`policy.rs::handle_admin_policy_get` with a call to the surviving
helper. Resolves: F-005. Default gates.

**1030 — Remove dead `installed_key` placeholder + simplify
`stage_name` suffix in rotate-key.** Delete
`stores.rs:862-863` (the two-line placeholder dance). Add a
`random_hex_8() -> String` (or similar) helper next to
`generate_master_key_bytes` and use it in `stage_name`. Resolves:
F-003, F-004. Default gates.

**1040 — Extract `random_id(prefix: &str) -> String` and have the
three callers use it.** Place in a new `src/ids.rs` module (or fold
into `users.rs` next to `new_user_id` — pick one). Replace
`generate_store_id`, `generate_client_instance_id`,
`generate_resource_id` with thin named wrappers (or inline). Update
imports. Resolves: F-006. Default gates.

**1050 — Clean stale ticket-number comments and "until then"
references.** Touch lines listed in F-009: `auth.rs:16-19`,
`admin_api/auth.rs:248-249`, `web.rs:205`, plus the `(ticket 212/214/240)`
field-rustdoc footnotes in `config.rs`, `runtime_state.rs`,
`auth.rs:72`. Replace each "until then" line with current state;
replace each `(ticket N)` footnote with a one-sentence *why*.
Resolves: F-009. Default gates.

**1060 — Rename `validate_store_name`.** Either rename to
`validate_nonempty_display` and move to a small `text` helper, or
inline the trim-and-check at the three call sites
(`stores.rs`, `client_instances.rs`). Either way the misleading
"store_name" name disappears. Resolves: F-010. Default gates.

**1070 — Split `auth.rs` inline tests to `auth/tests.rs`.** Move the
`#[cfg(test)] mod tests { … }` block (lines 407-700) to
`src/auth/tests.rs`. Leave production code untouched. Resolves:
F-007. Default gates.

**1080 — Split `users.rs`, `runtime_clients.rs`, `web.rs` inline
tests to sibling `tests.rs` files.** Same mechanical move as 1070
for each. Resolves: F-008. Default gates.

**1090 — Consolidate `AdminRuntimeState` test builders.** Add one
`default_admin_runtime_state()` helper that returns the canonical
defaults, then rewrite `test_admin_state`, `seeded_admin_state`,
`service_state_with_runtime_client`, `fresh_bootstrap_state`,
`crud_state` to call it and mutate only the fields they care about.
Aim to shrink `test_support.rs` from 753 lines toward ~500.
Resolves: F-011. Default gates.

**1100 — Resolve the `http/mod.rs` TODO.** Either move
`https_accept_loop`, `handle_https_connection`, `handle_https_request`
from `server.rs:142-207` to `http/mod.rs` (per the modularization
epic), or delete the trailing sentence in `http/mod.rs:11-13`. Ask
the user which way to go before starting. Resolves: F-012.
Default gates + `nix build .#checks.x86_64-linux.vm-test` if option 1
is chosen (the move touches the transport boundary).

**1110 — Decide and lock the public-API surface of the crate.**
Either lift the `pub use` block out of `server.rs` into `lib.rs` and
update the three external callers
(`crates/integration-tests/src/{harness/mod.rs,tests/daemon.rs}`,
`crates/backend-backendd/tests/ts_export.rs`, `src/main.rs:6`) to
import from `underwrap_backendd::*` directly, or keep `server.rs` as
the path of record and delete the "Once the crate root grows…"
comment so its purpose is clear. Ask the user which way to go.
Resolves: F-002. Default gates.

**1120 — Final simplification pass.** Re-read every file touched in
1010–1110 against the category 7a bullet list (needless wrappers,
defensive branches that can't fire, redundant clones, dead options,
re-implementations of stdlib, etc.). Remove anything that still
fits. Run all default gates plus the VM test. Resolves: trailing
simplification.

---

Approve the plan and start a fresh session — it will pick this
report up from `tickets/_review/` and create the tickets, then
execute them via `underwrap-ticket-workflow`.
