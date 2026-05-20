# Story: Rename "wrappers/grants" surface to "wrapper permissions"

The page currently labeled **Wrappers** at `/wrappers` is the surface
where operators set per-`(client_instance, wrapper, resource)`
permissions. Its real subject is **wrapper permissions**, not
wrappers. The term "agent grant" leaks through the codebase from the
project's early framing — agents were the original motivation, but
underwrap is a general-purpose secret brokerage, and the policy model
should be named accordingly.

This story renames the user-facing label, the Rust + TypeScript types,
the admin API endpoints, and sweeps the leftover "matrix" vocabulary
from comments and docs.

## Decisions (resolved)

- Page label: **"Wrapper permissions"**.
- Route path: **`/wrappers` stays** (no URL churn).
- Rust struct: `AgentGrant` → `WrapperPermission`.
- `PolicyDocument.grants` field → `permissions` (also changes the
  on-disk JSON shape of the policy blob — greenfield, no backcompat).
- Helper methods: `find_grant`, `evaluate_grant{,_at,_after_approval}`
  → `find_permission`, `evaluate_permission{…}`.
- API types: `AdminAgentGrant{s}{List,Upsert,Delete}{Request,Response}`
  → `AdminWrapperPermission{s}{…}`.
- API endpoints: `POST /v1/admin/grants/{list,upsert,delete}` →
  `POST /v1/admin/wrapper-permissions/{list,upsert,delete}`.
- Stable error code: `duplicate_grant` → `duplicate_permission`.
- "matrix" wording in comments / dialogs / docs → "wrapper
  permissions".

## Out of scope (do NOT rename)

- `docs/agents.md` — agents-as-motivation doc, separate concept.
- ssh-agent references (the signing protocol used by `PerCallApproval`).
- `WrapperApprovalMode`, `PerCallApproval`, `approval_key`,
  `SecretSlot` — already named neutrally.
- `POLICY_DOCUMENT_SECRET_ID` — internal storage key, not user-visible.
- The route string `/wrappers` (only the label changes).

## Child tasks

- `441_task_rename_rust_protocol_types_and_backend.md` — protocol
  types, backend handlers, endpoint paths, storage shape, integration
  tests.
- `442_task_rename_spa_to_wrapper_permissions.md` — regenerated TS
  types, SPA imports, page label, headings, dialog copy, vitest +
  playwright.
- `443_task_docs_sweep_for_wrapper_permissions.md` — `docs/*.md`,
  `CLAUDE.md`, JSON examples, terminology audit.

## Acceptance criteria (story-level)

- [ ] No reference to `AgentGrant`, `AdminAgentGrant*`, or
      `/v1/admin/grants/` remains in `crates/`, `web/`, `docs/`,
      `CLAUDE.md`, or `examples/`.
- [ ] No user-facing "matrix" copy remains in `web/` or `docs/`
      (excluding ticket history under `tickets/`).
- [ ] Page nav label, document title, and `<h2>` all read
      "Wrapper permissions".
- [ ] `cargo test --workspace`, `cd web && npm test`,
      `npm run test:e2e`, and
      `nix build .#checks.x86_64-linux.vm-test` all green.
