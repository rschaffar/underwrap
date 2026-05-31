# Refinement review: admin SPA — secrets + libsecret pages

Date: 2026-05-31
Reviewer session: claude-opus-4-8[1m]
Scope: The `secrets` page subtree of the admin SPA —
`web/src/pages/secrets/{page,resource-block,reveal-secret-view}.tsx`,
`grouping.ts`, the six KV dialogs under `dialogs/`, and the libsecret
subtree (`libsecret/{tree,collection-block,aliases-panel}.tsx`,
`helpers.ts`, and its six dialogs). This is the richest UI surface and the
one that reveals and writes raw secret material. Reviewed read-only.

## Summary

- 12 findings across 5 categories (Simplification, Dead code/API hygiene,
  Error handling, Doc/comment hygiene, Test coverage, Secret safety).
- 0 must-fix, 12 nice-to-have.
- Proposed ticket shape: **flat task tickets** (5 tasks, all small and
  independent) — under the 6-task / 200-LoC threshold for an epic.
- The code is clean on the greenfield axes: no ticket references, no
  compat shims, no legacy/migration code, no stale terminology.
- Every finding becomes a ticket. No deferral bucket.
- **Verification pass (claude-opus-4-8):** F-01 confirmed — `KvSecretSlot`
  is exactly `"env" | "files"` (`schema.ts:1136`; `SecretSlot = KvSecretSlot`
  in `grouping.ts:5`), so the third placeholder arm is unreachable. F-02's
  root cause confirmed in the shared `useFormDialog` (see shared-infra F-20).
  Findings stand as written.

Cross-component note: F-02 (redundant `as TValues` casts in `submit`
callbacks) is a symptom of the `useFormDialog` generic typing, whose root
fix lives in the **shared-infra** review. The cast-removal half is local
and is bundled into ticket 1100 here, sequenced after the hook fix.

## Findings

### F-01 — Unreachable third slot branch in `WriteSecretDialog`
- **Category:** Dead code & API hygiene / Simplification
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `web/src/pages/secrets/dialogs/write.tsx:110-115`, 120-122
- **Observation:** `SecretSlot = "env" | "files"` (schema.ts:1136), but the
  placeholder ternary carries a third arm `: "e.g. default/gh-cli.secret"`
  that can never fire.
- **Recommendation:** Collapse to a two-branch `if/else` and delete the
  dead arm; add a compile-time exhaustiveness check if the union ever grows.
- **Behavior change?** no

### F-02 — Redundant `as TValues` casts in `submit` callbacks
- **Category:** Simplification / Dead code & API hygiene
- **Severity:** nice-to-have
- **Scope:** in-component (root cause is shared `useFormDialog`)
- **Location(s):** `dialogs/write.tsx:57`, `dialogs/create-resource.tsx:40`,
  `dialogs/rename-resource.tsx:54`,
  `libsecret/dialogs/create-collection.tsx:43`,
  `libsecret/dialogs/create-item.tsx:59`,
  `libsecret/dialogs/set-alias.tsx:48`
- **Observation:** `useFormDialog<T>` already gives `submit` a `T`-typed
  argument; each `const x = v as XValues` is a no-op assertion masking the
  hook's `(values as unknown as TRequest)` coercion.
- **Recommendation:** After the shared-infra hook-typing fix lands, drop the
  six casts and destructure `v` directly.
- **Behavior change?** no

### F-03 — `useMemo` keyed on a freshly-built array defeats memoization
- **Category:** Simplification
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `web/src/pages/secrets/libsecret/aliases-panel.tsx:21-27`
- **Observation:** `entries = Object.entries(aliases)` is rebuilt every
  render and then listed in the `danglingTargets` `useMemo` deps, so the memo
  recomputes every render.
- **Recommendation:** Depend on `[aliases, collections]` and move
  `Object.entries(aliases)` inside the memo body.
- **Behavior change?** no

### F-04 — Redundant lambda wrappers around `confirm()` in four delete dialogs
- **Category:** Simplification
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `dialogs/delete-secret.tsx:37`,
  `libsecret/dialogs/delete-alias.tsx:36`,
  `libsecret/dialogs/delete-collection.tsx:37`,
  `libsecret/dialogs/delete-item.tsx:38`
- **Observation:** `onConfirm={() => confirm()}` — `confirm` (a
  `(args: void) => Promise<unknown>`) already satisfies `onConfirm`'s
  `() => unknown` type.
- **Recommendation:** Pass `onConfirm={confirm}` directly.
- **Behavior change?** no

### F-05 — Misleading "could not be decoded as UTF-8" error on reveal
- **Category:** Error handling / Doc & comment hygiene
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `web/src/pages/secrets/reveal-secret-view.tsx:104-111`
- **Observation:** `atob()` does Latin-1 base64 decode, not UTF-8 decode; the
  catch only fires on invalid base64. The message misrepresents both the step
  and the failure condition. Binary secrets with valid base64 decode to
  garbled Latin-1 rather than hitting this branch.
- **Recommendation:** Reword to name a server-side base64 problem; if proper
  UTF-8 display of binary secrets is wanted, switch `atob` → `TextDecoder`.
- **Behavior change?** no (message only)

### F-06 — No tests for any secrets dialog, `page.tsx`, or `grouping.ts`
- **Category:** Test layout & coverage
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `web/src/pages/secrets/dialogs/` (6 files),
  `libsecret/dialogs/` (6 files), `page.tsx`, `grouping.ts` — only
  `aliases-panel.test.tsx` and `tree.test.tsx` exist.
- **Observation:** Zero coverage for the write/rename path, duplicate-name
  detection in `CreateResourceDialog`/`CreateCollectionDialog`, the
  `DeleteResourceDialog` blocker list, `groupSecretEntries`, and the
  `SecretsPage` store-selector / empty-state branches.
- **Recommendation:** Add the five test groups listed in the ticket below;
  start with the pure `grouping.ts` functions.
- **Behavior change?** no

### F-07 — `LibsecretTree` defaults `enabled = true`, defeating the deferral prop
- **Category:** Dead code & API hygiene / Complexity
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `web/src/pages/secrets/libsecret/tree.tsx:24-34`
- **Observation:** The prop exists to defer the libsecret fetch until the
  `<details>` opens, but defaults to `true`, so a caller that forgets it
  fetches for every visible resource on mount. The tree tests omit the prop,
  masking the guarded path.
- **Recommendation:** Make the prop required (or default `false`); pass it
  explicitly in the tests so the guarded path is also covered.
- **Behavior change?** yes — callers omitting `enabled` stop auto-fetching;
  the sole production caller `ResourceBlock` always passes it explicitly, so
  production behaviour is unchanged.

### F-08 — `creating` state restates `Omit<SecretAddress, "key">` inline
- **Category:** Simplification / Naming
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `web/src/pages/secrets/page.tsx:81-83`, 206
- **Observation:** Anonymous `{ resource_id; slot }` state duplicates the
  shared `SecretAddress` shape; `{ ...creating, key: "" }` reconstructs it.
- **Recommendation:** Type the state as `Omit<SecretAddress, "key"> | null`.
- **Behavior change?** no

### F-09 — WHAT-comments on `summary` / `displayLabel`
- **Category:** Doc & comment hygiene
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `web/src/pages/secrets/resource-block.tsx:43-60`
- **Observation:** Both comments restate the adjacent line. Only the libsecret
  note (that the tree counts its own content) carries real information.
- **Recommendation:** Delete the `displayLabel` comment; trim `summary` to the
  one-sentence why.
- **Behavior change?** no

### F-10 — WHAT-comment on `ItemAttributes` rendering
- **Category:** Doc & comment hygiene
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `web/src/pages/secrets/libsecret/collection-block.tsx:132-135`
- **Observation:** "Render attributes inline, compact" restates the JSX; only
  the "list is normally short" note is useful.
- **Recommendation:** Keep only the why sentence.
- **Behavior change?** no

### F-11 — Reveal cache: no documented in-memory secret lifetime note
- **Category:** Logging & observability / Secret safety
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `web/src/pages/secrets/reveal-secret-view.tsx:51-56`
- **Observation:** `gcTime: 0` correctly collects the cache entry on unmount,
  but the secret transiently lives in the JS heap (query result + cache
  object) until GC; there is no active zero-fill. Acceptable for a browser
  SPA, but undocumented.
- **Recommendation:** Add a short comment stating the in-memory lifetime /
  no-zero-fill model so future maintainers understand the boundary.
- **Behavior change?** no

### F-12 — `helpers.ts` comment claims a `max 128` rule the validators don't apply
- **Category:** Doc & comment hygiene
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `web/src/pages/secrets/libsecret/helpers.ts:3-6`
- **Observation:** Comment documents `[A-Za-z0-9._-]+`, max 128, no leading
  `_`; `libsecretIdField()` / `optionalLibsecretIdField()` enforce the regex
  and prefix but not the length.
- **Recommendation:** Add `.max(128, …)` to both validators (cheap, closes
  the gap) — preferred over weakening the comment.
- **Behavior change?** yes — the client now rejects >128-char ids the backend
  would reject anyway; no valid input changes.

## Ticket plan

Flat tasks, number block 1100. Tasks 1110/1120/1130 are independent; 1100 is
sequenced after the shared-infra hook-typing fix.

### 1100_task_secrets_simplification_pass.md
- **Title:** Simplify secrets dialogs (dead branches, redundant casts/wrappers, memo, state typing)
- **Description:** Apply the same-behaviour simplifications across the
  secrets subtree: delete the unreachable slot branch (F-01), remove the six
  `as TValues` casts now that the shared `useFormDialog` typing is fixed
  (F-02), fix the `aliases-panel` `useMemo` deps (F-03), inline `confirm`
  in the four delete dialogs (F-04), and type the `page.tsx` `creating`
  state as `Omit<SecretAddress, "key">` (F-08). Do a final category-7a
  re-read of each touched file before closing.
- **Findings:** F-01, F-02, F-03, F-04, F-08
- **Sequencing:** depends on the shared-infra `useFormDialog`/`onSuccess`
  generic fix (shared-infra ticket) for the F-02 cast removals.
- **Acceptance:** `cd web && npm test` and `cd web && npm run build` green.

### 1110_task_secrets_libsecret_tree_enabled_default.md
- **Title:** Make `LibsecretTree.enabled` explicit (no auto-fetch default)
- **Description:** Drop the `enabled = true` default (make required or
  default `false`) so the fetch only fires when the parent expands; update
  the two tree tests to pass `enabled` explicitly and add a case for the
  `enabled=false` (no fetch) path.
- **Findings:** F-07
- **Acceptance:** `cd web && npm test` green (incl. new guarded-path case).

### 1120_task_secrets_error_and_comment_hygiene.md
- **Title:** Fix reveal error message, libsecret id length, and comment hygiene
- **Description:** Reword the reveal decode error to name a server-side
  base64 problem (F-05); add `.max(128)` to the libsecret id validators and
  reconcile the helper comment (F-12); add the in-memory-secret-lifetime note
  to the reveal cache config (F-11); delete/trim the WHAT-comments in
  `resource-block.tsx` (F-09) and `collection-block.tsx` (F-10).
- **Findings:** F-05, F-09, F-10, F-11, F-12
- **Acceptance:** `cd web && npm test` and `cd web && npm run build` green.

### 1130_task_secrets_dialog_and_grouping_tests.md
- **Title:** Add tests for secrets dialogs, page branches, and grouping
- **Description:** Cover the gaps in F-06: unit tests for `groupSecretEntries`
  / `resourceCount`; `CreateResourceDialog` duplicate-name rejection without a
  network call; `DeleteResourceDialog` blocker-list rendering;
  `WriteSecretDialog` create-vs-edit and key-rename paths; and the
  `SecretsPage` store-selector / empty-state branches.
- **Findings:** F-06
- **Acceptance:** `cd web && npm test` green with the new tests.

Approve the plan and start a fresh session — it will pick this report up from
`tickets/_review/` and create the tickets, then execute them via
`underwrap-ticket-workflow`.
