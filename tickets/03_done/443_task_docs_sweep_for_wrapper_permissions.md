# Task: Docs sweep — wrapper permissions terminology

Parent: `440_story_rename_to_wrapper_permissions.md`
Depends on: `441_task_rename_rust_protocol_types_and_backend.md` (for
the final endpoint paths and JSON field names to reference accurately).

Rewrite all operator- and developer-facing docs to use "wrapper
permission" instead of "agent grant" (when referring to the policy
model) and "wrapper permissions" instead of "matrix" (when referring
to the SPA page). Update JSON examples to the new field shape.

## Scope

### `docs/backend.md`

- Section heading "Agent grants" → "Wrapper permissions".
- Rewrite prose so it does not frame this as agent-specific. Example
  rewrites:
  - "on the grant decides whether the agent can use that grant right
    now" → "on the permission decides whether the runtime client can
    use it right now".
  - "To remove an agent's access to a resource" → "To remove a
    runtime client's access to a resource".
- Update endpoint references: `/v1/admin/grants/*` →
  `/v1/admin/wrapper-permissions/*`.
- Update JSON examples: `{"grants": […]}` → `{"permissions": […]}`.
- Update `duplicate_grant` → `duplicate_permission` in the error code
  table.
- Leave the ssh-agent / per-call-approval signing prose alone — only
  the policy-model vocabulary changes.

### `docs/admin-ui.md`

- Row at line 44 of the route table: `Matrix | /matrix | any operator`
  → `Wrapper permissions | /wrappers | any operator`.
- Section at lines ~52–54 ("The matrix tab lets admins set the…") —
  rewrite as "The Wrapper permissions tab lets admins set the…",
  using "permission" instead of "grant" throughout.

### Other docs

- `docs/admin-ui-nextjs-rfc.md` line 85: "access-matrix toggles" →
  "wrapper-permission toggles".
- `docs/backend-release-plan.md` lines ~173, ~182: "wrapper mode
  console and access matrix console" → "wrapper mode console and
  wrapper permissions console"; "matrix toggles" → "permission
  toggles".
- `docs/security.md`: grep for "grant" / "agent grant" / "matrix" and
  rename where the term refers to the policy model. **Leave
  agent-facing-workflow sections and ssh-agent prose alone.**

### `CLAUDE.md`

- "Grant model" paragraph: rewrite header and body to "Wrapper
  permission model". Update field references (`AgentGrant` →
  `WrapperPermission`; grants → permissions) and the
  `(wrapper, host, user)` description if needed to match the actual
  current key shape (`(client_instance, wrapper, resource)`).
- Key-files list: update the description of
  `crates/shared-protocol/src/policy.rs` to use the new type name.

### `examples/config.yaml`

- If any field, comment, or example references the old vocabulary,
  update it. (Most likely already neutral — confirm.)

### `docs/agents.md` — DO NOT TOUCH

This doc is intentionally about the agent-use-case motivation and
stays as-is.

## Verification

- [ ] `rg -i 'matrix' docs/ CLAUDE.md examples/` returns nothing.
- [ ] `rg -i 'agent[- _]?grant' docs/ CLAUDE.md examples/` returns
      nothing (matches inside `docs/agents.md` are acceptable only if
      they refer to agents-as-users, not to the policy model).
- [ ] Any JSON example block parses against the new
      `PolicyDocument` shape (eyeball check).
- [ ] All endpoint paths in docs match the renamed routes from 441.

## Out of scope

- Ticket history under `tickets/03_done/` — leave as-is (historical
  record).
- Code changes — covered by 441 and 442.
