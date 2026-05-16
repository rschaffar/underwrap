# Story: Agent grant model rewrite — remove per-secret access control

Parent: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Background

The current policy model has two problems that need to go:

1. **Per-secret mutation windows.** Operators currently "unlock" a single secret for mutation for a bounded duration (e.g. 30 minutes), then write it. This per-secret time-bounded state is too fine-grained, complicates the policy model, and leaks an ad-hoc state machine into both the daemon and the UI. See the now-obsolete design in `tickets/03_done/mutation-as-policy-state.md`.
2. **Per-secret read/write access control in the wrapper-agent matrix.** The current matrix toggles permissions at the (wrapper, principal, secret) granularity. This is wrong: agents need whatever secrets they need to function, and the right unit of authorization is the *agent grant* itself — `(wrapper, host, user)` — not individual secrets.

## Desired model

**Agent grants** live at the tuple `(wrapper, host, user)` — one row per (wrapper, principal) combination in the matrix. Each grant carries one of the four existing wrapper modes:

- `Locked` — agent cannot use this grant.
- `Unlocked` — agent can use this grant indefinitely.
- `UnlockedUntil { until }` — agent can use this grant until a deadline.
- `PerCallApproval { approval_key }` — every invocation requires a fresh ssh-agent signed approval (existing mechanism).

The mode controls whether the agent can use this grant *at all* right now.

**Read access** through a grant is unconditional: if the grant is active (mode says yes), the wrapper reads all the secrets its declaration lists. No per-secret read toggling. This matches how agents actually work — a wrapper like `gh-authorized` needs `gh-token` to do its job, and toggling that secret away from it would just break the wrapper.

**Write access** through a grant is governed by a **single global write-lock toggle**: `writes_enabled: bool`. When enabled, agents can write through any active grant (subject to grant mode). When disabled, no agent can write any secret, anywhere. No per-secret write ACL. No per-grant write ACL. One toggle, applied uniformly.

Per-secret unlock-until windows are removed entirely. The `Unlocked { until }`-style per-secret state machine in `crates/shared-protocol/src/policy.rs` and its UI surfaces are deleted.

## Out of scope for this story

- Auto-disabling the global write toggle after a duration. The toggle is manual; admin enables, does the work, disables. We can revisit auto-disable later.
- Per-secret write ACLs in any form. Not building the data structure means we cannot drift back into the fine-grained model by accident.
- Reworking the wrapper chain or the `PerCallApproval` ssh-agent challenge mechanism — kept as-is.

## Tasks

1. `245_task_remove_per_secret_mutation_windows_and_policy.md`
2. `246_task_grant_tuple_model_wrapper_host_user.md`
3. `247_task_global_write_lock_toggle.md`
4. `248_task_remove_mutation_endpoints_and_audit_variants.md`
5. `249_task_grant_model_test_coverage.md`

## Acceptance criteria

- [ ] Per-secret unlock-until state machine deleted from the policy types.
- [ ] Per-secret mutation endpoints removed; obsolete audit event variants removed.
- [ ] Grant model: one entry per `(wrapper, host, user)` with one of the 4 existing wrapper modes.
- [ ] Read access through an active grant is unconditional (no per-secret read filter).
- [ ] Global `writes_enabled` toggle governs all writes (admin UI + wrapper agents).
- [ ] Access matrix UI updated (delivered in ticket 250): rows = wrappers, columns = principals `(host, user)`, cells = mode or "not granted".
- [ ] Tests cover the 4 modes, global write-lock behavior, and that no per-secret ACL paths remain.
- [ ] `docs/security.md` updated to reflect the simpler grant model.
