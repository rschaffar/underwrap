# Task: Docs + CLAUDE.md — trust-model rewrite

Part of: `epic_single_instance_wrapper_redesign_2026-05`.
Depends on: every other ticket landing first (docs describe what
ships).

## Problem

The redesign changes the wrapper identity model. The docs and
`CLAUDE.md` currently describe — and lean hard on — the per-wrapper
setgid-group + per-socket two-factor identity, and state "Never
trust client-supplied wrapper names." That guidance is now wrong.
This is the central reviewer-facing change in the epic, so the docs
need real surgery, not a bolt-on section.

## What to do

### 1. `docs/security.md`

Rewrite the wrapper-identity section ("Wrapper identity: setgid +
effective GID", ~§ near line 314, plus the socket-permission notes
~line 672):

- **New model.** One setgid binary, one group, one socket per
  instance. Invocation is `uw-<instance> <wrapper-name> [args]` /
  `uw-<instance> debug <wrapper-name> [args]`.
- **What the kernel enforces.** A process must hold the instance
  group egid to open the `0660` socket; that egid is only obtainable
  by exec'ing the root-installed setgid binary. So raw secret access
  still requires going through the blessed binary.
- **What the daemon enforces.** The wrapper-name is client-supplied;
  the daemon validates it against config (rejecting unknown names
  and the reserved `debug`), and the **daemon chooses the target**
  (configured target for `Real`, debug tool for `Debug`). A user
  cannot run the debug tool under a `Real` authorization.
- **Why this doesn't weaken the model.** There was never per-user
  access control (the binaries are world-executable; any user could
  already invoke any wrapper). The per-wrapper group was only a
  which-wrapper disambiguator for the multi-socket design. The
  authorization gate is, and remains, the per-instance policy.
- **Debug variant.** Gated solely by `UnsafeDebugUntil`; exposes the
  projected secret material to the operator terminal; operators
  should pick the shortest TTL that fits the task.

### 2. `docs/backend.md`

- Add `variant: "real" | "debug"` to the open/finalize request
  schemas.
- Document the matrix: `UnsafeDebugUntil` allows both variants until
  expiry; every other mode denies `Debug`.
- Note the daemon-chosen-target rule as the reason `variant` is
  trustworthy end-to-end.

### 3. `examples/config.yaml`

Rewrite to the instance-level shape: one socket, one group, one
`debug_tool_target`, the `wrappers` map without per-wrapper
`socket`/`group`. Add a comment showing both call forms and noting
`debug` is reserved.

### 4. `CLAUDE.md`

- **"Never trust client-supplied wrapper names"** → replace with the
  new rule: the wrapper-name is a client-supplied selector,
  validated against config and gated by the instance group egid; the
  daemon (not the wrapper) chooses the target; the policy is the
  authorization authority.
- Update the **Identity model**, **Architecture notes**, and
  **Wrapper permission model** sections for the single-binary design
  and the `UnsafeDebugUntil` mode.
- Update the **Project structure** `debug-tool/` line: it is the
  target the daemon execs for `variant = Debug`, not an
  `addUnsafeDebugTool` install.

## Acceptance

- `grep -RIn "addUnsafeDebugTool" .` returns nothing.
- No doc still claims wrapper identity comes from a per-wrapper
  group or that client-supplied names are never trusted.
- A reader who hasn't seen the code can follow the new trust model
  from `docs/security.md` alone: kernel gate (instance group egid),
  daemon gate (name validation + target authority), policy gate
  (per-instance permissions incl. `UnsafeDebugUntil`).

## Resolution (2026-05-27) — done

`docs/security.md`: rewrote invariant #2 and §"Wrapper identity" for the
single-binary / one-socket / instance-group-egid model + SO_PEERPIDFD,
daemon-chosen target, the (mode × variant) matrix incl. UnsafeDebugUntil,
the stage-1/2 flow, the socket section, the "fixed target paths" →
daemon-chosen-target section, a "Target-host daemon hardening" subsection
(1470), and removed the stale `writes_enabled` "Mutation gating" toggle.
`docs/backend.md`: added `variant` to open/finalize, the four modes +
matrix, "No global write lock". `examples/config.yaml` was already in the
instance-level shape. `AGENTS.md`/CLAUDE.md: rewrote the
client-supplied-name rule, Identity model, Wrapper permission model,
runtime-API variant, debug-tool line. For end-to-end coherence also
updated `docs/architecture.md`, `docs/getting-started.md`, `README.md`,
`docs/agents.md`, `docs/gh-libsecret.md`, `docs/wrapper-author-checklist.md`
(work-jira → uw-work jira / per-user shims; removed per-call-approval).
`docs/backend-release-plan.md` left as an explicitly-historical doc.

Acceptance: `grep -RIn addUnsafeDebugTool` matches only ticket text;
no live doc claims per-wrapper-group identity or never-trust-names.
