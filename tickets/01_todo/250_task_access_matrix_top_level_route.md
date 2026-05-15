# Task: Move the access matrix to a top-level route (new grant model)

Parent: `250_story_access_matrix_tab_and_mobile_polish.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

Depends on `245_story_agent_grant_model_rewrite.md` being complete: the matrix UI is built against the new grant tuple model, not the per-secret matrix.

## Scope

- New SPA route `/matrix`, top-level navigation tab.
- Matrix axes:
  - **Rows: wrappers** (e.g. `gh-authorized`, `aws-cli-authorized`, …) — discovered from the policy document's wrapper declarations.
  - **Columns: principals** — each principal is a `(host, user)` pair (e.g. `deepthought / robert`). Discovered from the configured service clients of the backend and any grants already in the policy document.
  - **Cells: grant mode for the `(wrapper, host, user)` tuple**:
    - empty / "not granted" badge
    - `Locked` badge
    - `Unlocked` badge
    - `UnlockedUntil` badge with the deadline (with relative time tooltip)
    - `PerCallApproval` badge with a key fingerprint preview
- Cell click opens a small editor popover with:
  - Mode selector (4 buttons or a radio group)
  - For `UnlockedUntil`: a date/time picker
  - For `PerCallApproval`: ssh public-key textarea + fingerprint preview
  - "Remove grant" button → deletes the tuple via `DELETE /v1/admin/grants/{wrapper}/{host}/{user}`
- New-grant flow: cells showing "not granted" have a `+` affordance that opens the same popover preset to `Locked` (forces a deliberate mode choice).
- **Global write-lock toggle** rendered prominently at the top of the page:
  - Big switch labeled "Writes enabled globally" with the current state and last-changed metadata
  - When enabled, show an amber banner: "Writes enabled globally. Disable when finished."
  - Toggle calls `PUT /v1/admin/policy/writes-enabled`
- Filters (URL-persisted): wrapper (multi-select), principal (multi-select), store (single-select; defaults to first granted store, "All granted" for admin), mode (multi-select badge filter — e.g. "show me everything currently `Unlocked`").
- Inline updates use TanStack Query mutations with optimistic UI; rollback on backend error.
- Navigation order in the SPA shell: Overview / Resources / Wrappers / **Matrix** / Audit / (Users) / (Stores) / Settings.

## Out of scope (consistent with story 245)

- Any per-secret toggle in the matrix. The matrix never displays secret rows.
- Any per-grant write toggle. Write authorization is the global toggle only.

## Acceptance criteria

- [ ] `/matrix` accessible from the top-level nav.
- [ ] Rows = wrappers, columns = principals; no secret axis.
- [ ] Cell editor supports all 4 modes (Locked, Unlocked, UnlockedUntil, PerCallApproval) with appropriate sub-inputs.
- [ ] "Not granted" cells can be promoted to a grant via the `+` affordance.
- [ ] Global write-lock toggle visible and functional; banner shown when enabled.
- [ ] Filters persist across reload via URL params.
- [ ] Optimistic updates with rollback on backend error.
- [ ] Vitest covers filter-derivation logic and mode-badge rendering.
- [ ] Playwright covers: load matrix, cycle a cell through all 4 modes, set `UnlockedUntil` deadline, add per-call-approval key, remove grant, toggle global write lock, refresh-preserves-filters, optimistic-rollback on simulated backend error.
