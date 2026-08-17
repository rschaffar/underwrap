# Make unsafe-debug confirmation state the complete local security boundary

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-020
Source themes: C-005

## Outcome

Before an operator can select `UnsafeDebugUntil`, the confirmation names exactly what authority would be opened: the runtime client instance, store, wrapper/resource pair, pending expiry, full debug shell/exec projection, current writable/writeback effect, and the fact that **any local user** can invoke the system-wide frontend instance entry while the backend permission is active. It explicitly directs deployments that need per-user authorization or a prompt to put sudo, polkit, or another OS-level gate in front.

This is warning/confirmation correctness only. The backend permission model, default 15-minute deadline, explicit Save step, runtime debug target, writable enforcement, system-wide setgid entry, `allowedUsers` shims, and every authorization mechanism remain unchanged.

## Scope

- Replace the current brief unsafe-debug confirmation in `web/src/pages/wrapper-permissions/dialogs/edit-cell.tsx` with a structured warning that states:
  - **Local reach:** the frontend instance entry is system-wide; any local user can invoke it, not only the signed-in backend operator or users listed in `allowedUsers`; backend permissions identify the runtime client instance, wrapper, and resource, not a Unix user.
  - **Exact backend authority:** runtime client instance display name and id, store id, wrapper name, resource display name and id, and the `UnsafeDebugUntil` mode being selected.
  - **Full capability:** `underwrap-debug-tool` can show the env projection, open an interactive shell, or execute arbitrary commands with every configured env/files/libsecret projection available; that process can print, copy through accessible paths, or transmit the projection. underwrap is not a behavioral sandbox.
  - **Writable integrity:** when the current writable checkbox is enabled, files/libsecret changes can be submitted and persisted on successful finalization, affecting future invocations; env remains backend-owned/immutable. When writable is disabled, disclosure/full target access remains but underwrap does not persist staged changes.
  - **Time bound:** confirmation itself does not save backend state. It proposes one exact pending local expiry using the existing 15-minute default, displays that expiry and its default duration, and says the operator can review/change it before pressing **Save**. Backend expiry is rechecked on each request.
  - **OS gate:** use sudo, polkit, or another OS-level gate before saving when only selected local users should invoke or when a prompt is required. `allowedUsers` installs PATH shims only.
- Make the pending default deadline deterministic across the confirmation:
  - when another mode selects unsafe-debug, compute one pending `now + 15 minutes` local value and retain it while the danger dialog is open;
  - **Cancel** clears that pending value and leaves the original mode/deadline unchanged;
  - **Confirm** copies the exact displayed pending value into the deadline field and selects unsafe-debug;
  - adjusting the deadline while already in unsafe-debug does not re-prompt, as today;
  - selection/confirmation still does not call the backend until the operator presses **Save**.
- Keep the local frontend entry name honest: the backend UI knows the runtime client instance id/display name but not the Nix instance-entry name. Show the conventional `underwrap-<instance> debug <wrapper>` shape, identify the exact backend tuple separately, and tell the operator to verify the matching frontend instance before Save rather than inventing a name.
- Extend `web/src/pages/wrapper-permissions/dialogs/edit-cell.test.tsx` with deterministic fake-time evidence:
  - select unsafe-debug from another mode and assert the complete warning contains any-local-user/system-wide reach, exact client/store/pair identifiers, shell/exec/full projection, OS gate, and exact pending 15-minute expiry;
  - toggle writable before selection and assert the writeback/future-invocation integrity warning; separately assert the read-only wording does not imply persistence protection against disclosure;
  - cancel leaves original mode/deadline and makes no API call;
  - confirm copies the exact displayed expiry to the deadline input, closes the danger dialog, still makes no API call, and Save submits exact `unsafe-debug-until` epoch plus the current writable flag;
  - preserve no-reprompt-on-TTL-edit and existing non-debug mode/remove tests.
- Synchronize `docs/admin-ui.md` and `docs/security.md`:
  - admin guidance repeats the complete local reach/disclosure/writeback boundary and links to `security.md#untrusted-caller` for the OS-level gate;
  - security guidance explicitly ties `UnsafeDebugUntil` to the existing system-wide local caller model, full projection, writable integrity, exact pair/expiry, and no per-user authorization;
  - avoid duplicating the normative threat model beyond a concise task-oriented explanation/link.
- Preserve clean-HEAD evidence outside the repository:
  - desired-state script `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1190-unsafe-debug-warning-red.py`, SHA-256 `a808294412f3022f8ea0725d44f1511db713e2e76de043957709130227d0b9af`;
  - red log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1190-unsafe-debug-warning-red.log`, SHA-256 `40db658cdf57172cbba41761e811a5b666ef89ce241a45e5ed321f207fe16548`;
  - clean focused SPA baseline `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1190-web-baseline.log`, SHA-256 `5c69b827d78fccd4411fdd421710f0e482e843eff6cc8d26ee4bf2f166c65566`.
  The script exits 1: seven warning/test/doc conditions are absent while only the broad identifier-token inventory passes. The focused baseline passes all seven existing edit-cell tests.

Expected implementation/product paths are limited to:

- `web/src/pages/wrapper-permissions/dialogs/edit-cell.tsx`
- `web/src/pages/wrapper-permissions/dialogs/edit-cell.test.tsx`
- `docs/admin-ui.md`
- `docs/security.md`

A narrower path set is preferred if it satisfies every criterion. Required board moves and `tickets/_handover/codebase_2026-07-18.md` updates are workflow state, not product-scope expansion. Mandatory fresh cleanup discovery runs first; no cleanup is pre-approved. Formatting or dialog-component refactoring is not cleanup unless independently net-reducing and unrelated to the warning behavior.

## Non-goals

- Do not change backend permission modes, API/DTO/OpenAPI, expiry evaluation, writable enforcement, target selection, debug-tool behavior, setgid/socket permissions, `allowedUsers`, or the system-wide local entry model.
- Do not add per-user backend policy, local Unix identity to requests, sudo/polkit integration, prompts, allowlists, automatic permission re-lock, maximum TTL policy, or a second confirmation on Save.
- Do not claim unsafe debug is a behavioral sandbox, read-only mount, or protection against a target printing/persisting/transmitting its projection.
- Do not invent the frontend Nix instance name from backend display/id fields. Do not expose secret values in UI text, tests, logs, or fixtures.
- Do not absorb F-021 or any later finding, F-043, Opportunity, unresolved item, migration, or severity transition.

## Acceptance criteria

### Complete warning

- The danger dialog explicitly states any-local-user/system-wide invocation, backend-not-Unix-user authorization, exact runtime client/store/wrapper/resource tuple, conventional entry command shape, full env/files/libsecret shell/exec access, and OS-level sudo/polkit gating.
- It dynamically states the current writable effect: persisted files/libsecret writeback and future-invocation integrity impact when enabled; no persistence but unchanged disclosure/capability when disabled.
- It displays one exact pending local expiry corresponding to the existing 15-minute default and says mode activation occurs only after later Save.

### Confirmation behavior and evidence

- Cancel preserves the original mode and deadline and sends no request. Confirm applies exactly the displayed pending deadline locally and sends no request. Save sends the exact epoch/current writable body once.
- Existing unsafe mode TTL edits do not re-prompt. Existing ordinary permission modes, reason, remove, and API behavior remain unchanged.
- Focused SPA tests deterministically pin all warning statements, identifiers, expiry, cancel/confirm/Save sequencing, and writable branches. The desired-state script passes.

### Documentation and scope

- Admin UI guidance links the warning to the normative untrusted-caller/OS-gate threat model; security docs state the unsafe-debug-specific confidentiality/integrity direction without changing the model.
- No backend/API/protocol/Nix/privileged-chain/debug-tool/permission behavior, dependency, generated output, unrelated formatting, or adjacent F-id change.
- F-020 is resolved. F-021+, F-043, local-user authorization features, TTL policy, and every other finding/Opportunity remain separately queued.

## Security boundary

- Protected assets: confidentiality of the complete projected resource and integrity of files/libsecret values that writable finalization can persist for future invocations.
- Attacker: any local user, not merely the backend operator or an `allowedUsers` entry, can traverse the system-wide setgid instance entry while the intentionally unsafe backend permission is active.
- Boundary direction: backend admin confirmation → runtime-client/wrapper/resource `UnsafeDebugUntil` permission and expiry → system-wide local entry → daemon-chosen debug shell/exec target → projection disclosure and optional writeback.
- Invariant: the operator warning must name who can exercise the capability, the exact tuple/time bound, the full disclosure power, and writable integrity effect before the form can select unsafe debug.

## Validation

- Re-hash and preserve the red/baseline artifacts above; run the desired-state script after the fix.
- `cd web && npm test -- src/pages/wrapper-permissions/dialogs/edit-cell.test.tsx`
- `cd web && npm test && npm run build`; remove generated schema/dist afterward.
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- Static inventory of complete warning text, dynamic tuple/writable/deadline branches, no secret values, exact product path allowlist, and no generated/process artifacts.
- No standalone VM test is required for this warning-only UI/docs correction. Stop if implementation crosses NixOS module/service, local peer auth, setgid/socket permissions, target selection, or privileged wrapper chain.
- `nix flake check`; preserve exact output if the known clean-baseline LLVM 21 `llvm-profdata merge` SIGILL recurs after tests pass.

## Dependencies

- The accepted threat model and `docs/security.md#untrusted-caller` define system-wide reach, `allowedUsers`, and OS-level gating.
- Existing `UnsafeDebugUntil`, writable finalize, debug-tool shell/exec, and 15-minute UI default remain the behavioral authority.
- Completed F-006–F-008 define projection/finalization and debug target boundaries; F-017 preserves independent request scheduling. None changes here.
- No unresolved implementation dependency. Cleanup discovery begins automatically under D-111 after materialization; no cleanup unit is pre-approved.
