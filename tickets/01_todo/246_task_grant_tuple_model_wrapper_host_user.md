# Task: Grant tuple model — (wrapper, host, user) → mode

Parent: `245_story_agent_grant_model_rewrite.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Replace the current agent ACL representation with a flat list of `AgentGrant` per store:

  ```rust
  struct AgentGrant {
      wrapper: String,             // e.g. "gh-authorized"
      host: String,                // e.g. "deepthought"
      user: String,                // e.g. "robert"
      mode: WrapperApprovalMode,   // 4 existing variants
  }
  ```

- `(wrapper, host, user)` is the unique key; duplicates rejected on persist with a stable error code.
- Wrapper-side authorization path (in `crates/frontend-underwrapd/src/wrapper_flow.rs` and `approval.rs`):
  1. Daemon looks up the grant by `(wrapper, principal.host, principal.user)`.
  2. No grant → reject with 404 (same as the existing ungranted path).
  3. Grant exists → check `mode`:
     - `Locked` → reject
     - `Unlocked` → allow
     - `UnlockedUntil { until }` → allow if `now < until`, else reject
     - `PerCallApproval { approval_key }` → run the existing ssh-agent challenge flow
  4. Once the mode check passes, the daemon serves whatever secrets the wrapper's `secrets:` declaration lists. No per-secret filtering.
- Storage: serialize the grant list as part of the policy document under the existing reserved id (e.g. `__underwrap/catalog/document`), or rename to `__underwrap/admin/agent-grants`. Pick in implementation and document in the ticket close note.
- Admin API surface for grant CRUD:
  - `GET /v1/admin/grants` — list all grants for a store
  - `POST /v1/admin/grants` — create a grant
  - `PUT /v1/admin/grants/{wrapper}/{host}/{user}` — update mode
  - `DELETE /v1/admin/grants/{wrapper}/{host}/{user}` — remove a grant
- Every grant CRUD operation audited with operator principal and grant tuple.

## Key files likely involved

- `crates/shared-protocol/src/policy.rs`
- `crates/backend-backendd/src/server.rs`
- `crates/frontend-underwrapd/src/wrapper_flow.rs`
- `crates/frontend-underwrapd/src/approval.rs`

## Acceptance criteria

- [ ] Single source of truth for grants: a flat list per store.
- [ ] Daemon dispatch path gates only on grant mode; no per-secret state consulted.
- [ ] All 4 existing wrapper modes still work end-to-end via the wrapper chain.
- [ ] Grant CRUD endpoints work with role + store-grant guards (from tickets 213 / 241).
- [ ] Audit events on every grant CRUD include actor and grant tuple.
- [ ] Tests cover: each of the 4 modes, ungranted tuple → 404, duplicate tuple rejected, audit emission, grant CRUD admin-only and store-scoped.
