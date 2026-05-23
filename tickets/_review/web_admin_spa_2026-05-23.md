# Refinement review: web admin SPA

Date: 2026-05-23
Reviewer session: claude-opus-4-7 (Claude Code)
Scope: `web/src/**` — `main.tsx`, `App.tsx`, `router.tsx`, `api/client.ts`
(the auto-generated `api/schema.ts` is excluded from cleanup but read
for context), the shared shell in `components/app-shell.tsx`, the
shared `components/policy-viewer-dialog.tsx`, every shadcn primitive
under `components/ui/`, every page (`pages/dashboard.tsx`,
`pages/login.tsx`, `pages/bootstrap.tsx`, `pages/profile.tsx`, plus
the `stores/`, `users/`, `client-instances/`, `wrapper-permissions/`,
`secrets/` subtrees with their `dialogs/` children), the
`hooks/` (`use-theme.ts`, `use-store-selector.ts`,
`use-store-lookups.ts`), and the shared helpers in `lib/`. The
generated schema, the `e2e/` Playwright suite, and `coverage/` are
out of scope. The prior review
(`tickets/_review/admin_api_2026-05-23.md`) already covered the
backend handlers these pages call, so its findings are
cross-referenced rather than re-listed.

## Summary

- 16 findings across 6 categories. **0 must-fix**, 16 ticketed.
  Every finding becomes a ticket — no deferrals.
- The SPA is in good shape end-to-end: the typed `client.ts` wires
  every per-endpoint helper through a single `unwrap()` + CSRF
  middleware, the router has a small clean auth guard, and the
  per-page structure is consistent. What's left is finer-grained:
  three duplicated UI idioms that should become tiny shared
  components, two genuinely dead exports kept alive by tests,
  password / reserved-name rules that drift between pages, one
  stale "(legacy …)" comment, and a sweep that removes operator-name
  rejections everywhere now that on-disk artifacts are addressed by
  generated ids.
- Proposed ticket shape: **epic + story + flat task tickets**.
  Eight in-scope task tickets and one project-wide sweep ticket.

## Findings

### F-001 — Password length rule is split between SPA forms; backend only enforces non-empty

- **Category:** 2 — Naming & terminology / consistency
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):**
  - `web/src/pages/bootstrap.tsx:21` — `MIN_PASSWORD_LEN = 12`
  - `web/src/pages/profile.tsx:32` — `z.string().min(8, …)`
  - `web/src/pages/users/dialogs/create.tsx:42` — `z.string().min(8, …)`
  - `web/src/pages/users/dialogs/reset-password.tsx:34` — `z.string().min(8, …)`
  - `crates/backend-backendd/src/admin_api/users.rs:188-191, 275-278` —
    only `if body.password.is_empty()` rejects
  - `crates/backend-backendd/src/admin_api/auth/bootstrap.rs:86` and
    `auth/mod.rs:197` — OpenAPI advertises "password too short" but
    no length check exists in the handler
- **Observation:** Bootstrap requires ≥12, every other operator-facing
  form requires ≥8, and the backend only rejects empty. The OpenAPI
  description advertises a length rule the backend does not enforce.
- **Recommendation:** Move the rule to the **backend** (the authority
  for password handling). Add a single `MIN_PASSWORD_LEN = 12`
  constant to `crates/backend-backendd/src/auth.rs`, enforced by
  `hash_password` or by each write handler that consumes a raw
  password (`admin_api/auth/bootstrap.rs`,
  `admin_api/auth/mod.rs` password-change,
  `admin_api/users.rs` create + reset). Emit a stable
  `password_too_short` error code. Drop the SPA-side `.min(N)`
  constraints — `describeApiError` already maps the new code to a
  user-facing message.
- **Behavior change?** yes — current `≥8`-but-not-`≥12` passwords get
  rejected. Acceptance check must include the backend unit tests for
  the new code plus the SPA `client.test.ts` round-trip.
- **Related tickets:** none.

### F-002 — Operator-supplied name rejections everywhere; ids on disk are already generated

- **Category:** 7a — Simplification / 9 — Greenfield cleanups
- **Severity:** nice-to-have
- **Scope:** **project-wide** (backend handlers + SPA dialogs)
- **Location(s):**
  - `web/src/pages/secrets/dialogs/create-resource.tsx:33-42` —
    `__underwrap/` reserved, no leading/trailing slash, no
    env/files/libsecret slot segment in the path
  - `web/src/pages/secrets/dialogs/rename-resource.tsx:34-36` —
    `__underwrap` reserved (note the slash mismatch with create)
  - `web/src/pages/users/dialogs/create.tsx:36-41` — username regex
    `/^[a-z0-9][a-z0-9_-]*$/`
  - `web/src/pages/users/dialogs/edit.tsx:36-39` — same regex
  - `crates/backend-backendd/src/storage/secret_entries.rs:143-148`
    — `validate_and_compose_secret_id` rejects `resource_id`
    starting with `__underwrap`. The id is *generated*
    (`generate_resource_id()`); the check defends against a value
    the system cannot produce.
- **Observation:** Resource ids, store ids, user ids, and client-instance
  ids are all generated server-side (`generate_resource_id`,
  `fresh_id("store", …)`, `new_user_id`, `fresh_id("inst", …)`).
  Operator-supplied **names** are pure display labels — they do
  not pick filenames, URL path segments, or anything else that
  needs character-set gating. The reserved-name and "no slash"
  refinements are guarding against a world that no longer exists.
- **What stays:**
  - `shared-protocol/src/resource.rs:189-207` — `validate_env_name`
    (including the `LD_*` block). Env-slot keys become real process
    environment variables; the runtime security boundary is real.
  - `shared-protocol/src/resource.rs:96-111` — `validate_secret_key`
    for files/libsecret (path traversal, `.`/`..`, null bytes). The
    keys become path components in the bind-mounted directory and
    secretsd D-Bus paths; traversal still matters.
- **What goes:**
  - The four SPA refinements in `create-resource.tsx` and
    `rename-resource.tsx` (drop the `__underwrap`, slash, and
    slot-segment rules; keep "name must not be empty").
  - The username regex in the SPA users dialogs (drop entirely; the
    backend already normalises to lower+trim and rejects empty).
  - The `resource_id.starts_with("__underwrap")` rejection in
    `storage/secret_entries.rs:143-148` — it cannot fire and the
    documented "control over filenames" principle says operators
    don't pick filenames anyway.
- **Recommendation:** One sweeping ticket that removes the four
  SPA refinements, the username regex, and the dead `__underwrap`
  guard in `secret_entries.rs`. Re-read each call site against the
  category 7a bullet list before closing.
- **Behavior change?** yes — `__underwrap`-prefixed *display names*
  now go through. Since names are display-only and the backend's
  uniqueness check still applies, this is a UX change, not a
  security one.
- **Related tickets:** none.

### F-003 — `modeKind()` is a single-line passthrough used only in tests

- **Category:** 4 — Dead code & API hygiene
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):**
  - `web/src/pages/wrapper-permissions/derivations.ts:11-13`
  - `web/src/pages/wrapper-permissions.test.tsx:8, 115-120` (the only callsite)
- **Observation:** `modeKind(mode)` returns `mode.kind` — a
  tautology. Production code uses `existing?.mode?.kind` directly
  (`dialogs/edit-cell.tsx:48`). The export is only kept alive by a
  test that verifies the tag unwraps for each variant.
- **Recommendation:** Delete `modeKind` and the test that
  exercises it. Keep `ModeKind` as the alias for
  `WrapperApprovalMode["kind"]` (used by `edit-cell.tsx:25`).
- **Behavior change?** no.
- **Related tickets:** none.

### F-004 — `deriveClientInstances()` is unreachable in production

- **Category:** 4 — Dead code & API hygiene
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):**
  - `web/src/pages/wrapper-permissions/derivations.ts:26-30`
  - `web/src/pages/wrapper-permissions/page.tsx:69-79` — only caller,
    inside `storeName ? clientInstancesForStore(...) : deriveClientInstances(...)`
  - `web/src/pages/wrapper-permissions/page.tsx:108-124` — the page
    short-circuits to "No stores are visible" when `!storeName`,
    so the false branch never runs.
- **Observation:** The function only fires when `storeName` is
  falsy, but the page early-returns ten lines later in that case.
  The test file (`pages/wrapper-permissions.test.tsx:63-68`) is the
  only real caller.
- **Recommendation:** Delete `deriveClientInstances` and its test;
  simplify the call to a direct
  `clientInstancesForStore(storeName, …)`.
- **Behavior change?** no.
- **Related tickets:** none.

### F-005 — Page-level error rendering uses `<p role="alert">` while dialogs use `<Alert>`

- **Category:** 7 — Duplication / missed reuse
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):**
  - `web/src/pages/stores/page.tsx:57-59`
  - `web/src/pages/users/page.tsx:64-66`
  - `web/src/pages/client-instances/page.tsx:101-103`
  - `web/src/pages/wrapper-permissions/page.tsx:181-183`
  - `web/src/pages/secrets/page.tsx:165-167`
- **Observation:** Five pages inline
  `<p role="alert" className="text-sm text-rose-300">{describeApiError(error)}</p>`
  while dialog-level errors elsewhere use the shared `<Alert>`
  component. `Alert` already defaults `variant="destructive"` and
  sets `role="alert"`, so the inline paragraph reinvents the
  shadcn primitive.
- **Recommendation:** Replace each inline `<p role="alert">` with
  `<Alert>{describeApiError(...)}</Alert>`.
- **Behavior change?** no.
- **Related tickets:** none.

### F-006 — `document.title` setter duplicated across eight pages

- **Category:** 7 — Duplication / missed reuse
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):**
  - `web/src/pages/login.tsx:33-35`
  - `web/src/pages/bootstrap.tsx:44-46`
  - `web/src/pages/profile.tsx:42-44`
  - `web/src/pages/users/page.tsx:26-28`
  - `web/src/pages/client-instances/page.tsx:45-47`
  - `web/src/pages/stores/page.tsx:20-22`
  - `web/src/pages/secrets/page.tsx:34-36`
  - `web/src/pages/wrapper-permissions/page.tsx:33-35`
- **Observation:** Eight pages each carry
  `useEffect(() => { document.title = "underwrap — …"; }, []);`.
- **Recommendation:** Extract a tiny `useDocumentTitle(title: string)`
  hook in `web/src/hooks/use-document-title.ts` and call it from
  each page.
- **Behavior change?** no.
- **Related tickets:** none.

### F-007 — Client-instance and create-pair dialogs bypass the shared `Form`/`FormField` plumbing

- **Category:** 7 — Duplication / missed reuse
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):**
  - `web/src/pages/client-instances/dialogs/create.tsx:33-38`
  - `web/src/pages/client-instances/dialogs/edit.tsx:35-41`
  - `web/src/pages/wrapper-permissions/dialogs/create-pair.tsx:38-49`
  - Contrast: `web/src/pages/users/dialogs/create.tsx:60-67`,
    `web/src/pages/secrets/dialogs/create-resource.tsx:56-59`,
    every store dialog use
    `useForm({ resolver: zodResolver(schema) })`.
- **Observation:** Three dialogs were written before the Form
  pattern landed (or against a different convention) and manage
  their own validation, error placement, and accessibility wiring
  by hand. The cert-PEM textareas have no `<FormMessage>`; submit
  gating is hand-rolled
  (`displayName.trim() !== "" && storeId !== "" && trimmedCert !== ""`).
- **Recommendation:** Convert to the
  `useForm + zodResolver + <Form>` pattern. The cert-PEM textarea
  becomes a `<FormControl>` child so the `aria-invalid` /
  `aria-describedby` wiring lines up.
- **Behavior change?** no.
- **Related tickets:** none.

### F-008 — Confirm/delete dialogs hand-roll the same `submitError + Alert + describeApiError` triplet

- **Category:** 7 — Duplication / missed reuse
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):**
  - `web/src/pages/users/dialogs/delete.tsx:30-38`
  - `web/src/pages/client-instances/dialogs/delete.tsx:29-38`
  - `web/src/pages/client-instances/dialogs/set-status.tsx:31-39`
  - `web/src/pages/stores/dialogs/lock.tsx:33-41`
  - `web/src/pages/secrets/dialogs/delete-secret.tsx:29-40`
  - `web/src/pages/secrets/dialogs/delete-resource.tsx:29-32`
    (with blockers)
  - `web/src/pages/wrapper-permissions/dialogs/delete-pair.tsx:35-52`
    (with blockers)
- **Observation:** Seven confirmation modals: title, description,
  optional warning, error alert, cancel button, destructive button
  with pending state. Six of seven differ only in copy + mutation.
  The two delete-with-blockers dialogs add a blockers branch.
- **Recommendation:** Add a small `<ConfirmDialog>` wrapper to
  `web/src/components/` that takes
  `{ title, description, body?, destructive?, confirmLabel,
  pendingLabel, onConfirm, onClose }`. The two
  blockers dialogs pass `body` rather than re-implementing the
  shell. Each call site drops from ~70 lines to ~15.
- **Behavior change?** no.
- **Related tickets:** none.

### F-009 — `secrets/model.ts` sorts each slot by name in three near-identical lines

- **Category:** 7a — Simplification
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `web/src/pages/secrets/model.ts:51-55`
- **Observation:** Three identical sorts; the file already has the
  `SECRET_SLOTS` tuple at line 4.
- **Recommendation:**
  `for (const slot of SECRET_SLOTS) g[slot].sort((a, b) => a.key.localeCompare(b.key));`.
- **Behavior change?** no.
- **Related tickets:** none.

### F-010 — `resource-block.tsx` re-implements `normalizeEpoch` inline

- **Category:** 7a — Simplification / 7 — Duplication
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):**
  - `web/src/pages/secrets/resource-block.tsx:47-50`
  - `web/src/lib/epoch.ts:14-21` already provides `normalizeEpoch`.
- **Observation:** Inline
  `Number(provenance.created_at) ? new Date(...) : null` duplicates
  the shared helper.
- **Recommendation:** Use `normalizeEpoch(provenance?.created_at)`
  or `formatEpochAbsolute(provenance?.created_at)` directly.
- **Behavior change?** no.
- **Related tickets:** none.

### F-011 — `invalidatePairsAndPermissions` is a misnomer

- **Category:** 2 — Naming & terminology
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `web/src/pages/wrapper-permissions/page.tsx:101-106`
  and the apology comment at `:233-235`.
- **Observation:** The helper only invalidates the aggregate-view
  key. The name implies two distinct caches.
- **Recommendation:** Rename to `invalidateAggregateView`; drop
  the apology comment.
- **Behavior change?** no.
- **Related tickets:** none.

### F-012 — Stale "(legacy path-encoded ids never reach the wire)" comment

- **Category:** 9 — Greenfield cleanups
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `web/src/pages/secrets/model.ts:33-37`
- **Observation:** Per the greenfield rule, comments hedging
  against a previous shape are dead weight; no path-encoded id
  surface exists.
- **Recommendation:** Drop the parenthetical.
- **Behavior change?** no.
- **Related tickets:** none.

### F-013 — Dialog cache-invalidation pattern split across two conventions

- **Category:** 7 — Duplication / consistency
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):**
  - **Self-invalidating dialogs:**
    `web/src/pages/users/dialogs/create.tsx:58,70-72`,
    `web/src/pages/users/dialogs/delete.tsx:29,33-36`,
    `web/src/pages/stores/dialogs/create.tsx:49,69-71`,
    `web/src/pages/secrets/dialogs/write.tsx:62,108-111`, etc.
  - **Parent-driven invalidation:**
    `web/src/pages/client-instances/page.tsx:179-188` →
    `dialogs/create.tsx:43-48`, also `set-status.tsx`, `edit.tsx`,
    `delete.tsx`, plus
    `web/src/pages/wrapper-permissions/page.tsx:225-237` →
    `dialogs/create-pair.tsx:51-60`.
- **Observation:** Two valid patterns used inconsistently across
  the SPA.
- **Recommendation:** Convert the parent-driven dialogs to the
  **self-invalidating** pattern (the majority of the codebase
  already uses it; the dialog knows which keys it touched, which
  reduces the page's job to mounting/unmounting). Drop the
  `onSuccess` callback prop from the affected dialogs.
- **Behavior change?** no — same invalidation, different
  ownership.
- **Related tickets:** none.

### F-014 — `api/client.ts` carries a documentation contract that is no longer enforced

- **Category:** 10 — Doc & comment hygiene
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `web/src/api/client.ts:104-150`
- **Observation:** The "Named schema types" preamble says
  "Anything that's never referenced has been deleted — re-add only
  when a real callsite surfaces." The prior admin-API review
  pulled ~44 unused aliases. The remaining list is short, but the
  contract paragraph reads like a TODO for a discipline nobody
  owns.
- **Recommendation:** Replace with a one-line description ("Named
  aliases for the OpenAPI schemas referenced by hand. Generated
  entries that aren't used here live in `schema.ts`.").
- **Behavior change?** no.
- **Related tickets:** done/1900-1980 (the cleanup pass).

### F-015 — `PolicyViewerDialog` query key names a concept that no longer matches the endpoint

- **Category:** 2 — Naming & terminology
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `web/src/components/policy-viewer-dialog.tsx:41-43`
- **Observation:** Cache key is `["admin", "policy", storeName]`;
  the fetcher hits `/v1/admin/stores/{id}/wrapper-permissions`.
  Every other permissions-related query uses
  `["admin", "wrapper-permissions(-view)", storeName]`. No
  functional bug today (different endpoints, different caches), but
  the mismatch will trip the next refactor.
- **Recommendation:** Re-key to
  `["admin", "wrapper-permissions", storeName]`.
- **Behavior change?** no.
- **Related tickets:** none.

### F-016 — Inline `mod tests {}` blocks remain across Rust crates

- **Category:** 5 — Test layout
- **Severity:** nice-to-have
- **Scope:** **project-wide** (Rust crates)
- **Location(s):**
  - `crates/backend-backendd/src/main.rs:119` (27 lines)
  - `crates/frontend-secretsd/src/dbus_service.rs:98` (16 lines)
  - `crates/frontend-exec/src/main.rs:464` (19 lines)
  - `crates/shared-protocol/src/message.rs:16` (24 lines)
  - `crates/shared-protocol/src/service.rs:587` (15 lines)
  - `crates/frontend-underwrapd/src/main.rs:60` (26 lines)
  - `crates/frontend-underwrapd/src/storage.rs:6` (14 lines)
  - `crates/frontend-underwrapd/src/lib.rs:12` (9 lines)
- **Observation:** AGENTS.md mandates the sibling-`tests.rs`
  pattern without exception (the "tiny exception" language has
  been removed in this same change). Every inline block above is
  a violation.
- **Recommendation:** Relocate every inline `mod tests { ... }`
  block to a sibling `tests.rs`. No tests change shape — moves
  only.
- **Behavior change?** no.
- **Related tickets:** none.

## Ticket plan

Numbering: next free block is `2000_` (the recent `1900-1980` epic
landed in 7bf4759). One epic, one story, nine task tickets (eight
in scope plus one project-wide sweep). Every finding becomes a
ticket; nothing is deferred.

```
tickets/01_todo/
  epic_web_admin_spa_refinement.md
  2000_story_web_admin_spa_refinement.md
  2010_task_unify_inline_error_displays_and_document_title.md
  2020_task_extract_confirm_dialog_component.md
  2030_task_adopt_form_pattern_in_remaining_dialogs.md
  2040_task_remove_dead_modekind_and_derive_client_instances.md
  2050_task_simplify_secrets_and_provenance_helpers.md
  2060_task_tidy_stale_comments_and_naming_in_api_client_and_pages.md
  2070_task_remove_name_based_rejections.md
  2080_task_enforce_password_min_length_in_backend.md
  2090_task_self_invalidating_dialog_pattern.md
  2100_task_repo_sweep_relocate_all_inline_tests.md
```

(Ticket bodies for each of the above are created alongside this
report in the same session.)

End of report.
