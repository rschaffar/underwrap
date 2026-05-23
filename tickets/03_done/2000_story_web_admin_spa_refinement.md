# Story: SPA refinement pass — consolidate idioms and drop dead surface

Container for the nine task tickets under
`epic_web_admin_spa_refinement.md`. See that file for sequencing
and the cross-cutting cleanups (2070, 2080) that land alongside
the pure-SPA tickets.

The work is mechanical: extract shared helpers, convert the three
non-conforming dialogs, delete two dead exports, fix four naming /
comment / cache-key drifts, sweep operator-name rejections, and
move the password rule to the backend. No new features.

## Acceptance criteria

The story closes when every task ticket below is in
`tickets/03_done/`:

- 2010 — inline error displays + `useDocumentTitle`
- 2020 — `<ConfirmDialog>` extraction
- 2030 — Form pattern in remaining dialogs
- 2040 — drop `modeKind` and `deriveClientInstances`
- 2050 — secrets sort loop and provenance epoch helper reuse
- 2060 — name fixes in client/api/policy-viewer
- 2070 — remove name-based rejections everywhere
- 2080 — backend `MIN_PASSWORD_LEN`
- 2090 — self-invalidating dialogs

Plus the project-wide sweep:

- 2100 — relocate inline `mod tests {}` blocks
