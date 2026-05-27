# Execution plan — frontend hardening + single-instance wrapper redesign (2026-05)

This plan sequences two epics' worth of tickets currently in
`tickets/01_todo/`:

- `epic_frontend_security_hardening_2026-05` — tickets `1400`–`1480`
- `epic_single_instance_wrapper_redesign_2026-05` — tickets
  `1500`–`1580`

It exists so that after a session compaction the work can be picked
up and executed in the right order without re-deriving the
dependencies. Read this first, then the relevant epic, then the
ticket.

---

## CURRENT STATE — RESUME HERE (updated 2026-05-27)

### Phase 1 — DONE and committed (workspace + VM test green)

All hardening tickets are in `tickets/03_done/`. Committed as:

- `Frontend security hardening pass (tickets 1400/1410/1420/1430/1450/1480)`
- `Record resolution notes on tickets 1430 and 1480`

Note on **1430**: resolved *differently* than written. The premise was
wrong — NixOS `security.wrappers` deliberately strips `CAP_SETPCAP` from
the ambient set, so `underwrap-ns` never has it and `PR_CAPBSET_DROP`
always EPERMs. EPERM is kept non-fatal with an explanatory comment; the
real boundary is the final `capset` (permitted=0) + `PR_SET_NO_NEW_PRIVS`.
(Saved to auto-memory `project_nixos_wrapper_strips_setpcap`.) The
`cargo-fuzz` harness in 1480 was consciously deferred (nightly-toolchain
decision) — see `docs/secretsd-threat-notes.md`.

### Phase 2 — DONE (workspace + cargo tests green; VM test NOT yet run)

The full redesign bundle (1500/1510/1520/1530/1540) has landed. All five
ticket files are in `tickets/03_done/`. `cargo build`, `cargo test
--workspace` (22 test binaries, 0 failures), `cargo clippy --all-targets
--workspace -- -D warnings`, and `cargo fmt -- --check` are all clean.

What shipped:

- **1500/1510** — `shared-protocol`: instance-level `DaemonConfig`
  (`instance`, `socket`, `group`, `ns_binary`, `debug_tool_target`,
  `xdg_runtime_dir_base`), trimmed `WrapperDef`
  (`env_passthrough`/`env_set`, no socket/group/logical_name),
  `WrapperVariant`, `Read { wrapper_name, variant, scope }`,
  `ReadOk { …, target, env_* }`, `UnsafeDebugUntil`, `decide(permission,
  variant, now)`. `DaemonConfig::validate()` now also rejects forced env
  names in `env_passthrough` / `env_set` (moved from frontend-exec).
- **1530** — daemon binds ONE socket per instance gated by the single
  instance gid; intake checks `egid == instance gid`; `read.rs` validates
  the client `wrapper_name`, rejects the reserved `debug` token, and
  **the daemon picks the target binary** (configured `target` for Real,
  `config.debug_tool_target` for Debug) — emitted only after the backend
  allowed the variant. `variant` threads through `backend_client` and the
  resource channel's finalize calls.
- **1520** — `frontend-exec` is one per-instance binary. Identity from the
  `underwrap-<instance>` egid group; argv `[debug] <wrapper> [args]`;
  loads a minimal `/etc/underwrap/instances/<instance>.yaml` sidecar
  (socket + helper binary paths only); execs the **daemon-returned**
  target + env.
- **1540** — backend `resolve_invocation` takes `variant` and delegates to
  the shared `decide`; `decide`'s deny reasons are now the stable kebab
  tokens (`locked`/`expired`/`not-unsafe-debug`). `openapi.json` regenerated
  to include `WrapperVariant` + `UnsafeDebugUntil`. New matrix tests added.
- **frontend-keygen** now reads only the `backend` section of the instance
  config (via `serde_yaml::Value`) so it is decoupled from daemon-only
  schema fields.
- **integration-tests** harness uses one instance socket (the
  `wrapper_sockets` map points every name at it) with `group =
  current_group_name()` so the egid gate passes; all Read/ReadOk/runtime
  request sites carry `variant`.

The bundle is committable-green. **Next: Phase 3.** The VM test
(`nix build .#checks.x86_64-linux.vm-test`) was NOT run here — it depends
on the NixOS module rewrite (1550), which still emits the old per-wrapper
shape and will fail until 1550 lands. Run the VM test as part of Phase 3.

(Phase 3 then does 1550 NixOS module, 1470 systemd, 1560 SPA, 1440
SO_PEERPIDFD, 1570 VM test. Phase 4 = consolidated docs incl. 1580 +
the deferred doc bits.)

### Phase 3 — DONE (updated 2026-05-27, VM test green)

All Phase 3 tickets (1550, 1560, 1470, 1440, 1570) are in
`tickets/03_done/`. `nix build .#checks.x86_64-linux.vm-test` is green
(verified across three fresh runs), `cargo test --workspace`, clippy,
fmt all clean, and `cd web && npm test && npm run test:e2e` were green
in the 1560 pass. **Next: Phase 4** (consolidated docs — 1460 + 1580 +
the deferred doc bits for 1410/1420/1440/1470).

History (kept for context):

Done and committed:

- **1550** (NixOS module rewrite) — single `uw-<instance>` setgid binary,
  single `underwrap-<instance>` gate group (= daemon group), one socket,
  full `DaemonConfig` at `/etc/underwrap/daemon/<instance>.yaml`, minimal
  frontend-exec sidecar at `/etc/underwrap/instances/<instance>.yaml`,
  per-user `<wrapper>`/`debug-<wrapper>` shims + bare `uw` for the
  `default` instance, reserved-`debug` assertion, `addUnsafeDebugTool`
  removed. The NixOS system **evaluates** and the underwrap package
  **builds**. Ticket file still in `02_inprogress` because its VM-build
  acceptance is gated on 1570 (test.py still references the old shape).
- **1560** (admin SPA unsafe-debug-until) — DONE, in `03_done`.
  `ConfirmDangerDialog`, edit-cell confirm-gate, badge, vitest (68),
  `npm run build`, and the **full** Playwright suite (30) all green.
  Doing this also unblocked the package/VM build (the regenerated
  `schema.ts` had made the edit-cell switch non-exhaustive).
- **Full e2e modernization** — the whole Playwright suite was red from
  cross-cutting changes predating this epic (synchronizer-token CSRF +
  HttpOnly cookie, admin REST refactor, dashboard removed → `/`→`/secrets`,
  operator roles/grants removed, page renames). All specs updated; 30/30
  green. Shared `adminMutate` helper added to `web/e2e/fixtures.ts`.

Also done and committable-green (code complete; VM-acceptance gated on
1570, so the ticket files sit in `02_inprogress`):

- **1470** (systemd hardening + cert/key startup mode check) — added the
  remaining `serviceConfig` directives (`ProtectKernelLogs`,
  `ProtectProc=invisible`, `ProcSubset=pid`, `RestrictNamespaces`,
  empty `CapabilityBoundingSet`/`AmbientCapabilities`, `SystemCallFilter`
  with `@system-service ~@privileged @resources @chown`, explicit
  `ReadOnlyPaths`). `@chown` is re-allowed because the daemon chowns its
  wrapper socket. `frontend-keygen` now writes the cert `0600` (key was
  `0400`); new `startup::check_credential_permissions` refuses to start
  on a present-but-over-permissive cert/key/parent-dir. PKI test harness
  tightened to `0600`/`0700`.
- **1440** (`SO_PEERPIDFD` peer auth) — `peer_auth.rs` rewritten:
  race-free pidfd primary path (pid pinned across the `/proc` read),
  `SO_PEERCRED`+`/proc` fallback for pre-6.5 with a warn-once,
  uid/gid cross-check catching PID reuse, `PeerAuthPath` in the audit
  line, `log_peer_auth_mode()` startup probe, and a
  `hardenTargetHost ⇒ kernel ≥ 6.5` module assertion. No new dep.

`cargo test --workspace` (incl. integration-tests), clippy, fmt all
green; module parses.

Remaining in Phase 3: **1570** (VM test rework: `uw-<inst> [debug]
<wrapper>`, shims, one group/socket, target authority, expiry, spoofing,
plus 1470's cert-mode assertion and 1440's peer-auth path). After 1570,
run `nix build .#checks.x86_64-linux.vm-test` to close 1550 + 1470 +
1440 + 1570.

---

## Guiding constraints

- **No-legacy cutover.** Replace subsystems in one shot; never ship a
  half-migrated tree as an intermediate stable state. This is why the
  redesign's protocol+consumers land as one bundle (see Phase 2).
- **End result over half-finished.** Each phase must leave the
  workspace green (`cargo build`, `cargo test --workspace`, clippy,
  VM test). Do not stop mid-bundle.
- **Security tests are never deleted to pass.** Fix root causes.
- Move each ticket file `01_todo → 02_inprogress → 03_done` as you
  work it (per `CLAUDE.md` kanban convention).

## Per-ticket green-bar gates (run before marking a ticket done)

Unless a ticket says otherwise:

```bash
cargo fmt -- --check
cargo clippy --all-targets --workspace -- -D warnings
cargo test --workspace
```

Nix/module or VM-affecting tickets additionally:

```bash
nix flake check
nix build .#checks.x86_64-linux.vm-test
```

Web tickets:

```bash
cd web && npm test && npm run test:e2e   # e2e inside `nix develop`
```

**Exception:** the redesign bundle (Phase 2, tickets 1500–1540) is
*not* green per-ticket — those protocol changes break every consumer
until the whole bundle lands. The green bar applies to the bundle as
a whole. See `epic_single_instance_wrapper_redesign_2026-05`
"Landing strategy."

---

## Phase 1 — Independent hardening (do first, derisks the cutover)

These touch `frontend-ns`, `scm_rights`, and `frontend-secretsd` —
**zero file overlap** with the redesign. They land cleanly as
individual commits and reduce risk before the invasive Phase 2.
Order within the phase is by size/independence; they do not depend on
each other.

1. **`1430`** — `drop_caps` EPERM hard error. Tiny, isolated.
2. **`1450`** — `recv_fd` `MSG_CTRUNC` check. Tiny, isolated.
3. **`1400`** + **`1410`** — libsecret helper fd hygiene + helper
   `env_clear`. Do as a pair (same file, same exploit chain).
   - Note for 1410: it cites `env_set` as a PATH source. That value
     still reaches `frontend-ns` via the `ExecBundle`; unaffected by
     the redesign.
4. **`1420`** — remount `/proc` whenever `CLONE_NEWPID` is set.
   - Its `docs/security.md` PID-ns paragraph is the one `1460`
     also touches — defer that doc edit to Phase 4.
5. **`1480`** — secretsd D-Bus adversarial pass. Largest and most
   open-ended; independent of everything else, so it can run in
   parallel/background across the whole effort.

**Phase 1 exit:** all six green individually; workspace + VM test
green. (Defer the `docs/security.md` edits these tickets describe to
Phase 4 — draft them, but commit them in the consolidated pass.)

---

## Phase 2 — The redesign bundle (one cutover)

Tickets **`1500`, `1510`, `1520`, `1530`, `1540`** land **together**
on one branch. They change foundational `shared-protocol` types and
every Rust consumer; the workspace will not compile between them.

Suggested authoring order inside the bundle:

1. **`1500`** — shared-protocol: instance-level config (`socket`,
   `group`, `ns_binary`, `debug_tool_target`, `instance`,
   `xdg_runtime_dir_base`), trimmed `WrapperDef` (drop
   `socket`/`group`/`logical_name`, add `env_passthrough`/`env_set`),
   `WrapperRequest::Read { wrapper_name, variant, scope }`,
   `WrapperResponse::ReadOk { …, target, env_* }`, `WrapperVariant`,
   reserved-`debug` validation.
2. **`1510`** — shared-protocol policy: `UnsafeDebugUntil`, `variant`
   on runtime open/finalize, `decide` matrix.
3. **`1530`** — daemon: single socket bind + single instance gid;
   intake checks egid == instance gid, reads client `wrapper_name` +
   `variant`, validates against config (reject unknown / `debug`),
   relays to backend with `variant`, builds `ReadOk` with the
   **daemon-chosen** target (configured for Real, `debug_tool_target`
   for Debug) + env + `UNDERWRAP_DEBUG_*` metadata for Debug.
4. **`1520`** — frontend-exec: single per-instance binary, argv
   `[debug] <wrapper> [args]`, send `Read{wrapper_name, variant}`,
   exec ns with the daemon-returned target/env.
5. **`1540`** — backend: variant-aware `resolve_invocation` + finalize
   matrix; admin API round-trips `UnsafeDebugUntil`.

**Security crux to verify in the bundle:** a user-supplied
`variant = Real` can never cause the debug tool to be exec'd — the
daemon, not the wrapper, selects the target, and only emits
`debug_tool_target` after the backend allowed a `variant = Debug`
request.

**Phase 2 exit:** `cargo build`, `cargo test --workspace`, clippy all
green across the bundle. `integration-tests` cover both variants +
name validation on the single socket.

---

## Phase 3 — Redesign surfaces (Nix, SPA, VM) + adapted hardening

After the Rust bundle is in, these land as independent commits.

1. **`1550`** — NixOS module: one `uw-<instance>` setgid binary, one
   `underwrap-<instance>` group, one socket, one daemon config;
   default-instance `uw` symlink; drop per-wrapper
   groups/binaries/sockets, per-wrapper sidecars, and
   `addUnsafeDebugTool`; reserved-`debug` assertion. Update
   `node.nix`.
2. **`1470`** (adapted) — systemd unit hardening **on the rewritten
   module** + cert/key startup mode check. Sequenced here because it
   edits the module `1550` just rewrote. Capture
   `systemd-analyze security` before/after in the PR.
3. **`1540`-dependent `1560`** — admin SPA: regenerate
   `web/src/api/schema.ts`, add `unsafe-debug-until` radio +
   `ConfirmDangerDialog` (confirm on first selection, revert on
   cancel, no re-prompt on TTL edit), warning badge. SPA tests.
4. **`1440`** (adapted) — `SO_PEERPIDFD` peer auth, rebased onto
   `1530`'s intake, framed as the now-sole kernel gate. Sequenced
   after the bundle because it edits the same intake code.
5. **`1570`** — VM test rework: `uw-<inst> <wrapper>` and
   `uw-<inst> debug <wrapper>`; debug denied unless `UnsafeDebugUntil`;
   target-authority assertion; one group / one socket; unknown /
   `debug` name rejected; expiry; spoofing resistance on the single
   socket. Depends on 1520/1530/1540/1550 (and ideally 1440 so the
   peer-auth path under test is final).

**Phase 3 exit:** `nix flake check`, VM test, and web tests all
green.

---

## Phase 4 — Consolidated documentation pass — DONE (2026-05-27)

Both `1580` and `1460` are in `tickets/03_done/`, with the deferred doc
bits from `1410`/`1420`/`1440`/`1470` folded in. The pass covered
`docs/security.md`, `docs/backend.md`, `docs/architecture.md`,
`docs/getting-started.md`, `README.md`, `AGENTS.md` (= `CLAUDE.md`),
`docs/agents.md`, `docs/gh-libsecret.md`, and
`docs/wrapper-author-checklist.md`. `examples/config.yaml` was already in
the instance-level shape. `docs/backend-release-plan.md` is left as an
explicitly-marked historical document. No live doc still claims
per-wrapper-group identity, "never trust client-supplied names", the
`writes_enabled` toggle, or the removed per-call-approval mode; the
`(mode × variant)` matrix and `UnsafeDebugUntil` are documented in both
`security.md` and `backend.md`. **Both epics are now complete.**

History (the plan as written):

Many tickets describe edits to `docs/security.md`; doing them
piecemeal would have five tickets fighting over the same file. Do
them as **one coherent pass** here, reconciling:

- **`1580`** — the core trust-model rewrite: identity section
  (single binary/group/socket, client-supplied-but-validated
  wrapper-name, daemon-chosen target, instance-group egid as the
  kernel gate), `docs/backend.md` (`variant` field + matrix),
  `examples/config.yaml` (instance-level shape, `debug` reserved),
  and the `CLAUDE.md` security-critical rules (replace "never trust
  client-supplied wrapper names").
- **`1420`** doc bit — §"What the PID namespace contributes" now true
  for all modes.
- **`1460`** — passthrough trade-offs (`TMPDIR`, `SSH_AUTH_SOCK`,
  `DISPLAY`) + narrow-helper precondition + the PID-ns paragraph
  (reconciled with 1420).
- **`1440`** doc bit — §"Why effective GID instead of real GID?" →
  `SO_PEERPIDFD` primary, `SO_PEERCRED` fallback, kernel-version
  assumption.
- **`1470`** doc bit — §"Target-host daemon" systemd hardening +
  startup mode check.
- **`1410`** doc bit — helper env sanitisation paragraph.

**Documentation must end coherent end-to-end:** a reader who hasn't
seen the code should be able to follow the new trust model from
`docs/security.md` alone — kernel gate (instance-group egid, race-free
via `SO_PEERPIDFD`), daemon gate (name validation + target
authority), policy gate (per-instance permissions incl.
`UnsafeDebugUntil`) — with no surviving claim that wrapper identity
comes from a per-wrapper group or that client-supplied names are
never trusted.

**Phase 4 exit:** `grep -RIn "addUnsafeDebugTool" .` returns nothing;
no doc contradicts the shipped model; `CLAUDE.md` updated.

---

## Final acceptance (both epics closed)

- All tickets in `tickets/03_done/`.
- `cargo fmt -- --check`, `cargo clippy --all-targets --workspace -- -D warnings`,
  `cargo test --workspace` clean.
- `cd web && npm test && npm run test:e2e` clean.
- `nix build .#checks.x86_64-linux.vm-test` green with the reworked
  single-binary + debug-subcommand + `SO_PEERPIDFD` assertions.
- Manual smoke: `uw-<inst> gh-authorized …` runs normally;
  `uw-<inst> debug gh-authorized` is denied until an operator sets
  `UnsafeDebugUntil` (with the confirm modal), then prints the
  projected env; flipping back to `Locked` denies it again.
