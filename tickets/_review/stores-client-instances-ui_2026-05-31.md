# Refinement review: admin SPA — stores + client-instances pages

Date: 2026-05-31
Reviewer session: claude-opus-4-8[1m]
Scope: Two related entity-CRUD page subtrees of the admin SPA — `stores`
(`page.tsx`, `table.tsx`, and the eight dialogs: create, edit, delete, lock,
unlock, rotate-key, key-reveal, cleanup-hint) and `client-instances`
(`page.tsx`, `badges.tsx`, and dialogs create, edit, delete, set-status),
plus the sibling tests `stores.test.tsx` and `client-instances.test.tsx`.
Stores expose the only human-visible master-key surface (`key-reveal`,
`rotate-key`). Reviewed read-only.

## Summary

- 15 findings across 6 categories (Simplification, Dead code, Error handling,
  Duplication, Doc/comment hygiene, Test coverage).
- 2 must-fix (F-05, F-14), 13 nice-to-have.
- Proposed ticket shape: **flat task tickets** (5 tasks). The
  shared-component extraction touches several files but is small per file;
  no single task crosses ~200 LoC of movement, so it stays flat rather than
  an epic.
- Clean on greenfield axes: no ticket refs, no migration/compat code.
- Every finding becomes a ticket. No deferral bucket.
- **Verification pass (claude-opus-4-8):** all 16 findings checked against
  source. F-05 confirmed (Create button line 88 has no `disabled` guard). F-14
  corrected — `stores.test.tsx` already covers unlock/locked-badge/create/
  force-delete; the gap is the key *reveal* direction + rotate/cleanup/lock,
  re-scoped accordingly. All other findings stand as written.

## Findings

### F-01 — Type alias interleaved inside the import block
- **Category:** Doc & comment hygiene / Naming
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `stores/dialogs/create.tsx:18-22`
- **Observation:** A `type AdminStoreCreateResponse = …` declaration sits
  between two `import` statements.
- **Recommendation:** Move it below the imports (or inline the
  `components["schemas"][…]` index at its single use site).
- **Behavior change?** no

### F-02 — Dead `open` prop on `CreateStoreDialog`
- **Category:** Dead code & API hygiene / Simplification
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `stores/dialogs/create.tsx:37`; `stores/page.tsx:69-73`
- **Observation:** `CreateStoreDialog` is the only dialog accepting `open` as
  a prop; the call site is already guarded (`{createOpen ? <…/> : null}`), so
  `open` is always `true` when mounted. Every other dialog uses the
  guard-then-render pattern with a hardcoded `open`.
- **Recommendation:** Drop the `open` prop; hardcode `open` on its
  `AdminDialog`; match the call site to every other dialog.
- **Behavior change?** no

### F-03 — Redundant casts for already-typed `submit`/`toRequest` values
- **Category:** Simplification
- **Severity:** nice-to-have
- **Scope:** in-component (root cause in shared `useFormDialog`)
- **Location(s):** `stores/dialogs/edit.tsx:39`; `stores/dialogs/unlock.tsx:44`
- **Observation:** `(v as EditValues)` / `(v as UnlockValues)` — `v` is
  already typed `TValues` by the hook.
- **Recommendation:** Remove both casts after the shared-infra hook-typing
  fix.
- **Behavior change?** no

### F-04 — Unsafe `as` casts on `useFormDialog`/`useConfirmMutation` results
- **Category:** Simplification / Dead code & API hygiene
- **Severity:** nice-to-have
- **Scope:** in-component (root cause in shared hooks)
- **Location(s):** `stores/dialogs/create.tsx:61-63`;
  `stores/dialogs/rotate-key.tsx:40`
- **Observation:** `onSuccess`'s `result` is typed `unknown`, forcing
  `result as AdminStoreCreateResponse` and
  `(await confirm()) as { key_hex; warning }`.
- **Recommendation:** Parameterise the hooks' result type with a `TResult`
  generic (shared-infra) so callers get typed results without casting. Until
  then, guard with a runtime shape check before reading `key_hex`.
- **Behavior change?** no

### F-05 — Stores-query failure silently blocks Create/Edit client-instance dialogs
- **Category:** Error handling
- **Severity:** **must-fix**
- **Scope:** in-component
- **Location(s):** `client-instances/page.tsx:52-55`, 88-93, 176-188
- **Observation:** Both dialogs render only when `storesQuery.data` is
  present (`{creating && storesQuery.data ? … : null}`, lines 176/183). The
  "Create client instance" button (line 88) carries no `disabled` guard — its
  `onClick` just sets `creating = true` (line 90). So if the stores fetch
  errors or is still loading, clicking Create/Edit flips the state but the
  dialog never mounts: a silent no-op with no error or spinner.
- **Recommendation:** Surface `storesQuery.error` (banner/toast); while the
  query is pending/errored, set `disabled` on the Create button (line 88) and
  the per-row Edit buttons with an explanatory tooltip, rather than letting
  the click no-op.
- **Behavior change?** yes — operators see an explanation / disabled control
  instead of a silent no-op. Verified: the button has no current `disabled`
  prop, so this is a real reachable path on backend failure.

### F-06 — Triple no-op `form.reset` in `UnlockStoreDialog`
- **Category:** Simplification
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `stores/dialogs/unlock.tsx:46-48`, 54-56, 106-107
- **Observation:** The dialog is freshly mounted each open (page guards with
  `{unlocking ? … : null}`) with `defaultValues { key_hex: "" }`; all three
  `form.reset({ key_hex: "" })` calls (onSuccess, onClose, Cancel) fire right
  before unmount and have no observable effect.
- **Recommendation:** Remove all three resets; unmount clears the field.
- **Behavior change?** no

### F-07 — Wrong "override below" comments in delete/rotate dialogs
- **Category:** Doc & comment hygiene
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `stores/dialogs/delete.tsx:33`;
  `stores/dialogs/rotate-key.tsx:35`
- **Observation:** Both empty `onSuccess` bodies say "via the override below"
  but there is no override — the result is read from the awaited `confirm()`
  in `onDelete`/`handleRotate`.
- **Recommendation:** Replace with an accurate note (invalidation runs in
  `useConfirmMutation`; the caller reads the awaited `confirm()` result) or
  delete the comment.
- **Behavior change?** no

### F-08 — Inline store status badges vs extracted client-instance badges; mismatched pill shape
- **Category:** Duplication / missed reuse / Naming
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `stores/table.tsx:73-94`; `client-instances/badges.tsx:1-36`
- **Observation:** Stores render status chips inline with hardcoded Tailwind
  (`rounded-full`); client-instances extract `StatusBadge`/`AutoCreateBadge`
  (`rounded-md`). Same visual concept, different radius token, no shared
  component.
- **Recommendation:** Extract a shared `StatusChip` and unify the radius.
- **Behavior change?** no

### F-09 — Opaque `auto_create_wrapper_assets ? true : undefined` idiom
- **Category:** Simplification
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `client-instances/dialogs/create.tsx:58`
- **Observation:** The ternary maps `false → undefined` to omit the key; the
  backend treats absent as `false`, so the boolean can be passed directly.
- **Recommendation:** Pass the boolean (or `|| undefined`); add a one-line
  note if the omit-vs-false distinction is load-bearing.
- **Behavior change?** no

### F-10 — Duplicated auto-create checkbox block across create/edit
- **Category:** Duplication / missed reuse
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `client-instances/dialogs/create.tsx:148-178`;
  `client-instances/dialogs/edit.tsx:158-184`
- **Observation:** The checkbox + label + `FormDescription` block is
  copy-pasted; only the `data-testid` and a one-sentence description differ.
- **Recommendation:** Extract an `AutoCreateField` component taking
  `control`, `testId`, and optional extended description.
- **Behavior change?** no

### F-11 — Duplicated cert-PEM `<textarea>` across create/edit
- **Category:** Duplication / missed reuse
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `client-instances/dialogs/create.tsx:136-145`;
  `client-instances/dialogs/edit.tsx:139-144`
- **Observation:** Identical `<textarea>` (rows, className, placeholder,
  spellCheck) in both dialogs; the project has no shadcn `Textarea` wrapper.
- **Recommendation:** Add `web/src/components/ui/textarea.tsx` (mirroring the
  `Input` wrapper) and use it in both.
- **Behavior change?** no

### F-12 — `stores.test.tsx` uses fragile positional button indexing
- **Category:** Test layout & coverage
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `stores.test.tsx:89-92`, 125
- **Observation:** `buttons[0]`/`buttons[1]` depend on which action flags are
  set and break silently if a new action button is added.
- **Recommendation:** Select by accessible name
  (`within(row).getByRole("button", { name: /^edit$/i })`, etc.), matching
  the technique used elsewhere in the file.
- **Behavior change?** no

### F-13 — Client-instance delete and re-enable flows untested
- **Category:** Test layout & coverage
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `client-instances.test.tsx` (whole file)
- **Observation:** No case exercises `DeleteClientInstanceDialog` /
  `deleteClientInstance`, nor the Re-enable path on a disabled instance.
- **Recommendation:** Add a delete test (asserts `deleteClientInstance` with
  the right id) and a re-enable test (asserts `putClientInstanceUpdate` with
  `{ status: "active" }`).
- **Behavior change?** no

### F-14 — Key-reveal / rotate-key / cleanup-hint flows untested
- **Category:** Test layout & coverage
- **Severity:** **must-fix**
- **Scope:** in-component
- **Location(s):** `stores.test.tsx` (existing cases at :80, :95, :112, :135,
  :158, :201)
- **Observation (corrected after verification):** The file *does* cover the
  unlock path (`postStoreUnlock` at :158-200), the locked badge + Unlock
  button (:135), create-dialog open (:95), and force-delete confirmation
  (:112). The genuine gaps are the security-sensitive **reveal** direction
  and its neighbours: no case renders `KeyRevealDialog` with the `key_hex`
  the server returns from create-encrypted or rotate-key, verifies it stays
  non-dismissible until the ack checkbox is ticked, or that `onClose` is gated
  on acknowledgement. The rotate-key flow, the post-force-delete
  `CleanupHintDialog`, and the Lock (as opposed to Unlock) dialog are also
  untested.
- **Recommendation:** Add cases for (a) create-encrypted-store → `key_hex`
  rendered in `KeyRevealDialog`, ack gates dismiss; (b) rotate-key → same;
  (c) force-delete → `CleanupHintDialog` renders; (d) Lock dialog. This is the
  only human-visible master-key surface; a regression here could leak or hide
  the key.
- **Behavior change?** no

### F-15 — Dead SSR guard in `KeyRevealDialog.onCopy`
- **Category:** Simplification / Greenfield cleanups
- **Severity:** nice-to-have
- **Scope:** in-component
- **Location(s):** `stores/dialogs/key-reveal.tsx:36`
- **Observation:** `typeof navigator !== "undefined" && navigator.clipboard`
  — an SSR guard in a pure SPA; under jsdom `navigator.clipboard` is
  `undefined` and is already handled by the inner check.
- **Recommendation:** Reduce to `if (navigator.clipboard) { … }`.
- **Behavior change?** no

## Ticket plan

Flat tasks, number block 1300. 1300 (must-fix error handling) is independent;
1310 simplifications depend on the shared-infra hook-typing fix for F-03/F-04
casts; 1340 (key-reveal tests, must-fix) is independent and high priority.

### 1300_task_client_instances_surface_stores_query_errors.md
- **Title:** Surface stores-query errors/loading on the client-instances page
- **Description:** Stop the silent no-op (F-05): show `storesQuery.error` in a
  banner/toast and reflect the loading state on the Create/Edit buttons so the
  operator always gets feedback.
- **Findings:** F-05 (must-fix)
- **Acceptance:** `cd web && npm test` and `cd web && npm run build` green;
  add a test asserting the error surfaces when the stores query fails.

### 1310_task_stores_client_instances_simplification.md
- **Title:** Simplify stores/client-instances dialogs (casts, dead props, reset, comments)
- **Description:** Move the interleaved type alias (F-01); drop the dead
  `open` prop on `CreateStoreDialog` (F-02); remove the value casts (F-03)
  and result casts/guards (F-04) after the shared-hook fix; delete the triple
  `form.reset` (F-06); fix the delete/rotate comments (F-07); pass the
  auto-create boolean directly (F-09); drop the SSR clipboard guard (F-15).
  Final category-7a re-read of each touched file.
- **Findings:** F-01, F-02, F-03, F-04, F-06, F-07, F-09, F-15
- **Sequencing:** depends on shared-infra hook-typing fix for F-03/F-04.
- **Acceptance:** `cd web && npm test` and `cd web && npm run build` green.

### 1320_task_extract_shared_entity_dialog_components.md
- **Title:** Extract shared StatusChip, Textarea, and AutoCreateField components
- **Description:** Unify the duplicated UI: a shared `StatusChip` for store
  and client-instance status badges with one radius token (F-08); a
  `components/ui/textarea.tsx` wrapper used by both client-instance dialogs
  (F-11); an `AutoCreateField` component used by create and edit (F-10).
- **Findings:** F-08, F-10, F-11
- **Acceptance:** `cd web && npm test` and `cd web && npm run build` green; no
  visual regression in the stores/client-instances tests.

### 1330_task_client_instances_delete_reenable_and_store_button_tests.md
- **Title:** Add client-instance delete/re-enable tests; harden store button selectors
- **Description:** Add delete and re-enable tests for client-instances
  (F-13); replace positional `buttons[i]` indexing with role+name selectors
  in `stores.test.tsx` (F-12).
- **Findings:** F-12, F-13
- **Acceptance:** `cd web && npm test` green with the new/updated tests.

### 1340_task_store_key_reveal_tests.md
- **Title:** Test the key-reveal / rotate-key / cleanup-hint / lock flows
- **Description:** Cover the security-sensitive master-key UI gaps that remain
  after the existing unlock/create/delete coverage (F-14): create-encrypted →
  `KeyRevealDialog` renders the returned `key_hex` + ack gates dismiss;
  rotate-key → same; force-delete → `CleanupHintDialog`; Lock dialog. Reuse
  the existing `fetchMock` harness pattern already in `stores.test.tsx`.
- **Findings:** F-14 (must-fix)
- **Acceptance:** `cd web && npm test` green; do not weaken the ack-gate
  assertions to make a test pass.

Approve the plan and start a fresh session — it will pick this report up from
`tickets/_review/` and create the tickets, then execute them via
`underwrap-ticket-workflow`.
