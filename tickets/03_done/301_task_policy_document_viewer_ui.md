# Task: Read-only policy document viewer in the admin SPA

Standalone ticket. Smaller scope than ticket 300 but in the same
"API exposed, no UI surface" category.

## Background

The backend exposes `POST /v1/admin/policy/get` returning the full
`PolicyDocument` for a store (grants + `writes_enabled` flag). The
SPA already calls `POST /v1/admin/policy/put` indirectly via the
Matrix page's grant editor. But there is no way to *view* the raw
policy document — the Matrix page renders a derived view (grant
tuples by `(wrapper, host, user)`) and hides the underlying shape.

Operators occasionally need to inspect the actual stored policy:
- To confirm what was persisted after a Matrix edit.
- To audit a store's full grant set without the Matrix's filtering.
- To debug a discrepancy between expected and observed authz.

## Scope

Add a read-only policy viewer somewhere in the SPA. Decide between:

- **Sub-page under Stores**: `/stores/<name>/policy` showing the
  raw `PolicyDocument` as formatted YAML or JSON, plus a small
  badge for `writes_enabled`.
- **Tab on the Matrix page**: a "Raw policy" tab next to the
  current matrix view.

Either choice is fine — pick whichever fits the existing nav
better. The viewer must be:
- Read-only. Editing stays in the Matrix flow.
- Per-store (a store selector or store-scoped route).
- Copy-friendly (a "copy to clipboard" affordance on the rendered
  document).

## Acceptance criteria

- [ ] Operators can view the raw policy document for any store they
      have admin access to.
- [ ] `writes_enabled` is shown prominently (it's the kill-switch
      for admin writes).
- [ ] At least one unit test covers the rendering with a non-trivial
      policy (multiple grants, mixed modes).
- [ ] Either a Playwright e2e covers the viewer, or its lack of
      e2e is explicitly noted as acceptable in this ticket close-out
      because the page is read-only and trivial.

## Out of scope

- Editing the document from this view — that's a deliberate scope
  boundary so the Matrix stays the single source of truth for
  mutation.
- Per-grant audit history. If that's wanted, file a separate
  ticket.
