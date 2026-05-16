# Task: Decide and (optionally) build the Secrets admin UI

Standalone ticket — gates a meaningful scope decision before the
first release.

## Decision (2026-05-16)

**Option A — full CRUD in the SPA.** A `/secrets` page exists as of
the same commit that closes this ticket, with list / reveal /
create / update / delete dialogs. Reveal is gated by an explicit
confirmation step and writes a server-side audit event. Create /
update / delete inherit the per-store `writes_enabled` toggle for
the destructive paths; reveal stays available regardless.

Rationale: underwrap brokers secrets. A first-release admin UI that
can't view or manage its primary data model would be hard to defend
to operators. The reveal-with-confirmation + audit pattern keeps the
threat-model honest about clear-text values landing in browser
memory. Sub-tickets in the original spec were folded into this one
implementation since the surface is small enough to ship as a unit.

## Background

The backend exposes a full secrets CRUD admin API:

- `POST /v1/admin/secret/list`
- `POST /v1/admin/secret/read`
- `POST /v1/admin/secret/write`
- `POST /v1/admin/secret/delete`

The TypeScript bindings for these requests exist in `web/src/api/`,
but **no page in the SPA calls them**. There is currently no way to
list, view, write, or delete a secret from the admin UI. Operators
must use the backend's other tooling (CLI, raw HTTP) for any
secret-level interaction.

For a product whose primary purpose is *brokering secrets*, this is
the largest visible gap in the admin surface. It may be intentional
(CLI-only by design — secrets are sensitive enough that an in-browser
viewer is a misfeature), or it may simply not have been scoped into
the rework. The decision belongs in this ticket.

## Scope

### Part 1 — decision

Decide explicitly, and record the rationale in this ticket:

- **Option A: ship a Secrets page in v1.** Operators can list,
  reveal (with confirmation), write, and delete secrets through
  the SPA. Requires careful UX around reveal-on-click + audit.
- **Option B: keep secrets CLI-only in v1.** Document this in
  `docs/backend.md`. Remove or mark the unused
  `AdminSecret*Request` TypeScript bindings as "API-only, no UI by
  design" so the next person reading the api/ layer doesn't waste
  time wondering.
- **Option C: list-only.** Operators can see *which* secrets exist
  (catalog) but not read/write/delete from the SPA.

The decision must consider:
- Is a clear-text secret value in a browser viewport acceptable on
  the threat model docs/security.md describes? What about a value
  copied to clipboard from there?
- Do declarative stores need a different surface than dynamic ones?
- How does this interact with the per-call-approval grant mode
  (would a SPA-side read trigger a per-call approval ping)?

### Part 2 — implementation (only if Option A or C)

If the decision is A or C, this ticket is split into follow-up
sub-tasks at that point:

- `30X_task_secrets_list_page.md` — table view + filter by store.
- `30X_task_secret_detail_view.md` — read-with-confirmation, audit
  hook, copy-to-clipboard UX.
- `30X_task_secret_write_dialog.md` — create/update with the same
  write-gate UX as Stores.
- `30X_task_secret_delete_with_confirmation.md`.
- `30X_task_secrets_e2e_coverage.md`.

If Option B, this ticket completes with just the docs and api/
cleanup steps.

## Acceptance criteria

- [ ] A decision is recorded in this ticket body (option chosen +
      one paragraph rationale).
- [ ] If Option B: `docs/backend.md` documents the CLI-only stance;
      the unused TypeScript bindings carry a comment explaining why
      they're there.
- [ ] If Option A or C: follow-up sub-tasks exist under
      `tickets/01_todo/` with concrete scope.

## Out of scope

- Implementing any of Option A/C until the decision is made.
