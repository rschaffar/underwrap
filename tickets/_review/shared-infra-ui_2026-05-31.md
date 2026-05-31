# Refinement review: admin SPA — shared infrastructure

Date: 2026-05-31
Reviewer session: claude-opus-4-8[1m]
Scope: The cross-cutting plumbing every page depends on — `api/client.ts`
(typed admin client + CSRF middleware), `router.tsx` (TanStack Router +
auth/bootstrap guard), `main.tsx`, the seven `hooks/`, the four `lib/`
helpers, and the shared `components/` (`app-shell`, `confirm-dialog`,
`ConfirmDangerDialog`, `admin-dialog`, `policy-viewer-dialog`), plus their
sibling tests. The vendored `components/ui/*` shadcn primitives and the
generated `api/schema.ts` were read for context but not reviewed for style.
Reviewed read-only.

## Summary

- 20 findings across 7 categories (Module boundaries, Naming, Error handling,
  Dead code/API hygiene, Test coverage, Duplication, Simplification,
  Doc/comment hygiene, Greenfield cleanups).
- 2 must-fix (F-04, F-05), 18 nice-to-have.
- Proposed ticket shape: **epic + story + 8 tasks**. Triggered both by count
  (>6 tasks) and by F-01 (the `client.ts` split is >200 LoC of movement).
- This component is the **root** of several cross-report findings: F-20 (hook
  generic typing) is the cause of the redundant casts flagged in the secrets,
  wrapper-permissions, and stores reviews; F-02/F-03 (the `ConfirmDangerDialog`
  rename) also resolves wrapper-permissions F-08. Those page-side tickets are
  sequenced after the matching tasks here.
- Greenfield axes: one stale migration comment (F-13) and one "(yet)"
  deferral marker (F-18); no ticket references anywhere in `web/`.
- Every finding becomes a ticket. No deferral bucket.
- **Verification pass (claude-opus-4-8):** F-05 confirmed at `router.tsx:103`
  (`!== undefined`). F-04 confirmed — `safeNextPath` defined at :258, its only
  caller is the login route at :195, and there is no `router.test.*`. F-20
  confirmed by reading `use-form-dialog.ts` (`submit: (request: TRequest) =>
  Promise<unknown>`, `onSuccess: (result: unknown)`; the `submit`-arg casts in
  the pages are already redundant under the `TRequest = TValues` default — the
  real gap is the `unknown` result). All findings stand as written.

## Findings

### F-01 — `client.ts` mixes plumbing with a 40+-entry endpoint registry
- **Category:** Module boundaries & file size
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `api/client.ts:1-203` (plumbing), 205-239 (re-exports),
  241-585 (named wrappers) — 585 LoC total.
- **Observation:** Lines 241-585 are 40+ one-line wrappers
  (`fetchStores`, `postStoreCreate`, `deleteLibsecretItem`, …) that only
  forward to `api.X`. Every new endpoint touches this one file.
- **Recommendation:** Split the wrappers by domain into co-located
  `api/{stores,secrets,client-instances,…}.ts` (or one `api/endpoints.ts`);
  keep `client.ts` ≤ ~200 LoC (types + CSRF middleware + the `api` surface).
- **Behavior change?** no

### F-02 — `ConfirmDangerDialog` PascalCase filename breaks the kebab convention
- **Category:** Naming & terminology
- **Severity:** nice-to-have
- **Scope:** project-wide
- **Location(s):** `components/ConfirmDangerDialog.tsx`,
  `components/ConfirmDangerDialog.test.tsx`; import at
  `pages/wrapper-permissions/dialogs/edit-cell.tsx:6`
- **Observation:** Every other component is kebab-case; this is the only
  PascalCase file, creating a case-sensitive import inconsistency on Linux.
- **Recommendation:** Rename to `confirm-danger-dialog.tsx` (+ test); update
  the single import.
- **Behavior change?** no

### F-03 — Two confirm dialogs with incompatible `open` contracts
- **Category:** Naming & terminology / Duplication
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `components/confirm-dialog.tsx:42`;
  `components/ConfirmDangerDialog.tsx:9,33`; caller `edit-cell.tsx:286`
- **Observation:** `ConfirmDialog` hardcodes `open` and relies on conditional
  mounting (11 callers); `ConfirmDangerDialog` takes an `open: boolean` and is
  mounted persistently by its single caller. The two also diverge on feature
  set (`pending`/`submitError`/`disabled`/`body` vs none). Keeping a separate
  lightweight danger dialog is fine, but the `open` handling should match.
- **Recommendation:** Remove `open` from `ConfirmDangerDialog`; switch its
  caller to conditional mounting, matching the established pattern.
- **Behavior change?** no

### F-04 — `safeNextPath` (open-redirect defence) has zero tests
- **Category:** Test layout & coverage
- **Severity:** **must-fix**
- **Scope:** in-component
- **Location(s):** `router.tsx:258-267`; `?next=` redirect at 120-121
- **Observation:** `safeNextPath` is the only guard against open-redirect
  after sign-in, is exported, and has no test. The `bootstrapRoute` `?next=`
  branch is silently ignored, so part of the function is never exercised.
- **Recommendation:** Add `router.test.ts` unit cases: same-origin path
  (pass), cross-origin URL (block), `..` traversal (block), non-string
  (block), `null` (null). Pure string transform — no router setup needed.
- **Behavior change?** no

### F-05 — Auth guard admits `{ username: null }`
- **Category:** Error handling
- **Severity:** **must-fix**
- **Scope:** in-component
- **Location(s):** `router.tsx:103`
- **Observation:** `if (principal.username !== undefined)` — the schema is
  `username?: string | null` (schema.ts:1183). A `{ username: null }`
  response would be treated as authenticated.
- **Recommendation:** Use `if (principal.username != null)` (catches both
  `null` and `undefined`).
- **Behavior change?** yes — correctly rejects `{ username: null }` as
  unauthenticated; the backend intends to omit the field, so no legitimate
  authenticated response changes.

### F-06 — Dead SSR branch in `client.ts` base-URL derivation
- **Category:** Simplification
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `api/client.ts:119-122`
- **Observation:** `typeof window !== "undefined" ? window.location.origin : ""`
  — no SSR pass exists (the comment says so); the else branch is dead.
- **Recommendation:** Use `const apiBaseUrl = window.location.origin;`; drop
  the comment.
- **Behavior change?** no

### F-07 — `useClientInstanceDirectory` iterates `query.data?.items` twice
- **Category:** Simplification / Duplication
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `hooks/use-client-instance-directory.ts:24,28`
- **Observation:** The `useMemo` re-reads `query.data?.items` instead of the
  already-derived `clientInstances`, and its dep array is only `[query.data]`.
- **Recommendation:** Iterate `clientInstances` inside the memo and align the
  dep array.
- **Behavior change?** no

### F-08 — `clearError` exported from `useConfirmMutation` but never called
- **Category:** Dead code & API hygiene
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `hooks/use-confirm-mutation.ts:21,54`
- **Observation:** No caller uses `clearError`; `confirm()` already clears the
  error at its top (line 49).
- **Recommendation:** Remove it from the interface and the return.
- **Behavior change?** no

### F-09 — `NAV` and `unwrap` exported with no external consumers
- **Category:** Dead code & API hygiene
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `components/app-shell.tsx:38`; `api/client.ts:142`
- **Observation:** `NAV` is used only by `Sidebar` in the same file; `unwrap`
  only inside `client.ts`.
- **Recommendation:** Drop the `export` on both.
- **Behavior change?** no

### F-10 — Duplicated invalidation block across the two hooks
- **Category:** Duplication / missed reuse
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `hooks/use-form-dialog.ts:63-68`;
  `hooks/use-confirm-mutation.ts:38-42`
- **Observation:** Both hooks contain the identical
  `Promise.all(invalidate.map(key => qc.invalidateQueries({ queryKey: key })))`.
- **Recommendation:** Extract `invalidateAll(qc, keys)` into `lib/query.ts`;
  import from both.
- **Behavior change?** no

### F-11 — Stale JSDoc claims a writable-permission count in `PolicyViewerDialog`
- **Category:** Doc & comment hygiene
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `components/policy-viewer-dialog.tsx:21`
- **Observation:** JSDoc says "plus a quick count of writable permissions",
  but the component renders only a JSON `<pre>` and a copy button.
- **Recommendation:** Delete the clause.
- **Behavior change?** no

### F-12 — Stale test description references a non-existent badge
- **Category:** Test layout & coverage / Doc & comment hygiene
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `components/policy-viewer-dialog.test.tsx:9`
- **Observation:** The `it(...)` description mentions a "writable-permissions
  badge"; the body asserts only on the JSON and copy button.
- **Recommendation:** Rename to describe what is actually asserted.
- **Behavior change?** no

### F-13 — Comment narrates a completed `policy/get → wrapper-permissions/list` migration
- **Category:** Greenfield cleanups / Doc & comment hygiene
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `components/policy-viewer-dialog.tsx:33-36`
- **Observation:** There is no `/policy` endpoint; the comment is historical
  archaeology of a switch that already happened.
- **Recommendation:** Replace with the present-tense rationale (wrap in
  `{permissions}` so the JSON matches the `PolicyDocument` shape operators
  recognise).
- **Behavior change?** no

### F-14 — `client.test.ts` test asserts only the third-party library shape
- **Category:** Test layout & coverage
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `api/client.test.ts:141-144`
- **Observation:** "exposes the raw openapi-fetch client" asserts
  `typeof client.GET === "function"` — testing `openapi-fetch`, not project
  code, and incentivising the `client` export (see F-09).
- **Recommendation:** Delete the test (and unexport `client` if nothing else
  needs it).
- **Behavior change?** no

### F-15 — Router uses the pre-v1 `new Route` / `new RootRoute` constructor API
- **Category:** Greenfield cleanups / Complexity
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `router.tsx:49,97,135,143,149,155,164,173,179,191,214,230,285`
- **Observation:** On `@tanstack/react-router@^1.169.2`, the documented v1 API
  is `createRootRoute` / `createRoute`; the constructor forms are the carried
  pre-v1 shape. A first release is the moment to align.
- **Recommendation:** Mechanical rename to the functional API; route tree and
  type registration are unchanged.
- **Behavior change?** no

### F-16 — `cn()` called with a single static string
- **Category:** Simplification
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `components/app-shell.tsx:79-81`
- **Observation:** `cn("sticky top-0 …")` with one unconditional literal runs
  `clsx` + `tailwind-merge` for nothing.
- **Recommendation:** Use a plain `className="…"` string.
- **Behavior change?** no

### F-17 — `window.setTimeout` should be plain `setTimeout`
- **Category:** Simplification
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `components/policy-viewer-dialog.tsx:47` (also
  `pages/secrets/reveal-secret-view.tsx:73`, fixed in the same pass)
- **Observation:** The `window.` qualifier is unnecessary in a browser SPA.
- **Recommendation:** Use `setTimeout(...)`.
- **Behavior change?** no

### F-18 — `use-theme.ts` "(yet)" deferral marker
- **Category:** Greenfield cleanups / Doc & comment hygiene
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `hooks/use-theme.ts:16`
- **Observation:** "No system-preference detection (yet)" reads as an
  intentional-shortcut marker.
- **Recommendation:** Either implement `prefers-color-scheme` (a trivial
  `matchMedia` read in `readInitial`) or reword to state the current design
  without "(yet)".
- **Behavior change?** no (reword) / yes-but-additive (if implemented).

### F-19 — `ConfirmDialog` and the auth guard `beforeLoad` are untested
- **Category:** Test layout & coverage
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `components/confirm-dialog.tsx` (no sibling test);
  `router.tsx:97-125` (no test)
- **Observation:** `ConfirmDialog` is the primary confirm primitive (11 call
  sites) yet has no test, while `ConfirmDangerDialog` does. The
  `appLayoutRoute.beforeLoad` guard — the only code on every protected
  navigation, including the `fetchWhoami → fetchBootstrapStatus` fallback and
  redirect logic — is exercised only in production.
- **Recommendation:** Add `confirm-dialog.test.tsx` (title/description/body,
  pending disables button, error renders `<Alert>`, cancel fires). Add
  `beforeLoad` branch tests in `router.test.ts`: authenticated → principal;
  unauthenticated + bootstrap required → `/bootstrap`; unauthenticated +
  bootstrap not required → `/login`.
- **Behavior change?** no

### F-20 — `useFormDialog`/`useConfirmMutation` generics leak `unknown`, forcing call-site casts
- **Category:** Dead code & API hygiene / Simplification
- **Severity:** nice-to-have
- **Scope:** in-component (root cause of cast findings in the page reviews)
- **Location(s):** `hooks/use-form-dialog.ts:24,32,52`;
  `hooks/use-confirm-mutation.ts` (`confirm` return / `onSuccess`)
- **Observation:** `submit` is typed `(request: TRequest) => Promise<unknown>`
  and `onSuccess` receives `result: unknown`. The `unknown` result forces
  `result as AdminStoreCreateResponse` (stores create) and
  `(await confirm()) as { key_hex; warning }` (rotate-key). Separately, the
  `submit`-argument casts in the page dialogs (`v as XValues`) are already
  redundant today because with `TRequest = TValues` the parameter is typed
  `TValues` — they are noise that obscures the real `unknown` gap.
- **Recommendation:** Add a `TResult` generic so
  `submit: () => Promise<TResult>` and `onSuccess: (result: TResult) => void`
  (and the equivalent on `useConfirmMutation`'s `confirm`). This removes the
  result casts. Once landed, the page-side simplification tickets (secrets
  1100, wrapper-permissions 1200, stores 1310) drop their now-redundant
  argument casts.
- **Behavior change?** no

## Ticket plan

Epic shape, number block 1400. Sequencing: 1430 (hook typing) should land
before the page-side cast-removal tickets in the other three reports; 1470
(client.ts split) is the large mechanical move; 1460 also resolves
wrapper-permissions F-08.

### epic_admin_shared_infra_refinement.md
- **Title:** Admin SPA shared-infrastructure refinement
- **Description:** Tighten the cross-cutting plumbing of the admin SPA for the
  first release: cover the two security-adjacent gaps (auth guard, open-redirect
  defence), fix the hook generic typing that forces casts across every page,
  split the oversized API client, unify the confirm-dialog naming/contract,
  and clear dead code / stale comments.
- **Children:** 1410–1480 below.

### 1405_story_admin_shared_infra_refinement.md
- **Title:** Shared-infra refinement work items
- **Description:** Story grouping the eight tasks; carries the cross-report
  sequencing notes (hook-typing fix and `ConfirmDangerDialog` rename are
  prerequisites for tickets in the secrets, wrapper-permissions, and stores
  reports).

### 1410_task_auth_guard_reject_null_username.md
- **Title:** Auth guard: reject `{ username: null }` as unauthenticated
- **Description:** Change `router.tsx:103` to `!= null` so a null username is
  treated as unauthenticated (F-05).
- **Findings:** F-05 (must-fix)
- **Acceptance:** `cd web && npm test`, `cd web && npm run build`, and
  `cd web && npm run test:e2e` green (auth navigation path).

### 1420_task_router_and_confirm_dialog_tests.md
- **Title:** Test `safeNextPath`, the auth-guard branches, and `ConfirmDialog`
- **Description:** Add `router.test.ts` covering `safeNextPath` (F-04,
  must-fix) and the `beforeLoad` branches (F-19); add `confirm-dialog.test.tsx`
  (F-19).
- **Findings:** F-04 (must-fix), F-19
- **Acceptance:** `cd web && npm test` green with the new tests; do not weaken
  the open-redirect assertions to make a test pass.

### 1430_task_hook_typing_and_invalidation_dedupe.md
- **Title:** Type hook results, dedupe invalidation, drop dead hook surface
- **Description:** Add the `TResult` generic to `useFormDialog`/
  `useConfirmMutation` so callers get typed results (F-20); extract a shared
  `invalidateAll` into `lib/query.ts` (F-10); remove the unused `clearError`
  (F-08); fix the double iteration / dep array in
  `useClientInstanceDirectory` (F-07).
- **Findings:** F-20, F-10, F-08, F-07
- **Sequencing:** prerequisite for the cast-removal work in secrets 1100,
  wrapper-permissions 1200, and stores 1310.
- **Acceptance:** `cd web && npm test` and `cd web && npm run build` green.

### 1440_task_shared_infra_dead_code_and_micro_simplification.md
- **Title:** Remove dead code and micro-simplifications in shared infra
- **Description:** Drop the dead SSR base-URL branch (F-06); unexport `NAV`
  and `unwrap` (F-09); delete the library-only `client.test.ts` case (F-14);
  replace single-arg `cn()` with a plain string (F-16); use plain
  `setTimeout` in `policy-viewer-dialog.tsx` and `reveal-secret-view.tsx`
  (F-17). Final category-7a re-read of touched files.
- **Findings:** F-06, F-09, F-14, F-16, F-17
- **Acceptance:** `cd web && npm test` and `cd web && npm run build` green.

### 1450_task_shared_infra_doc_and_comment_hygiene.md
- **Title:** Fix stale docs/comments in shared infra
- **Description:** Drop the writable-count clause from the policy-viewer JSDoc
  (F-11) and the matching test description (F-12); rewrite the
  `policy/get → wrapper-permissions/list` migration comment in present tense
  (F-13); resolve the `use-theme` "(yet)" marker (F-18) by rewording (or
  implementing `prefers-color-scheme` if desired).
- **Findings:** F-11, F-12, F-13, F-18
- **Acceptance:** `cd web && npm test` and `cd web && npm run build` green.

### 1460_task_rename_and_normalize_confirm_danger_dialog.md
- **Title:** Rename `ConfirmDangerDialog` to kebab-case and normalize `open`
- **Description:** Rename `ConfirmDangerDialog.tsx`/`.test.tsx` to
  `confirm-danger-dialog.tsx` and update the `edit-cell.tsx:6` import (F-02,
  resolves wrapper-permissions F-08); remove its `open` prop and switch the
  caller to conditional mounting (F-03).
- **Findings:** F-02, F-03 (resolves wrapper-permissions F-08)
- **Acceptance:** `cd web && npm test` and `cd web && npm run build` green.

### 1470_task_split_api_client_by_domain.md
- **Title:** Split the API client into plumbing + per-domain endpoint modules
- **Description:** Move the 40+ named endpoint wrappers out of `client.ts`
  into co-located per-domain modules (`api/stores.ts`, `api/secrets.ts`, …),
  leaving `client.ts` ≤ ~200 LoC of types + CSRF middleware + the `api`
  surface (F-01). Update all importers.
- **Findings:** F-01
- **Acceptance:** `cd web && npm run build` (tsc resolves every moved import)
  and `cd web && npm test` green.

### 1480_task_migrate_router_to_functional_api.md
- **Title:** Migrate router to TanStack `createRootRoute`/`createRoute`
- **Description:** Mechanical rename of the `new RootRoute`/`new Route`
  constructor calls to the v1 functional API (F-15); route tree and type
  registration unchanged.
- **Findings:** F-15
- **Acceptance:** `cd web && npm test`, `cd web && npm run build`, and
  `cd web && npm run test:e2e` green (navigation unchanged).

Approve the plan and start a fresh session — it will pick this report up from
`tickets/_review/` and create the tickets, then execute them via
`underwrap-ticket-workflow`.
