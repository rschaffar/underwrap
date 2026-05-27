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

### Phase 2 — IN PROGRESS (workspace does NOT compile)

Committed WIP: `WIP redesign bundle: shared-protocol foundation (1500 + 1510)`
(commit `583b8e4`).

- **1500 DONE**, **1510 DONE** — `shared-protocol` is green *in isolation*
  (`cargo fmt/clippy/test -p shared-protocol`). The crate compiles; the
  **workspace does not**, because the consumer crates still reference
  removed `WrapperDef` fields and the old `Read`/`ReadOk`/runtime-request
  shapes. This is the expected mid-cutover state.

**REMAINING Phase 2 work — do these next, then the workspace compiles
again:**

1. **1530 — daemon (`frontend-underwrapd`). THE SECURITY CRUX.**
   - Bind ONE socket per instance (`config.socket`) owned by
     `config.group` gid (mode 0660). Drop the per-wrapper socket-bind
     loop and `build_wrapper_gid_map` (now a single gid). Touch
     `server.rs` (bind loop ~189-198), `startup.rs`
     (`build_wrapper_gid_map` 10-30, `validate_config` 52-95, `bind_socket`
     125-160).
   - Intake (`wrapper_flow/intake.rs` `authorize_wrapper_peer` 15-64):
     check peer egid == instance gid; then read the client `Read`
     request, validate `wrapper_name` against `config.wrappers` (reject
     unknown and the reserved `debug` token), relay `variant` to the
     backend.
   - `wrapper_flow/mod.rs:42` matches `Read { scope }` — now
     `Read { wrapper_name, variant, scope }`.
   - `wrapper_flow/read.rs` (`open_invocation` ~70-173): drop
     `logical_name` fallback (use the client `wrapper_name`, already
     validated); pass `variant` to `backend_client`; build `ReadOk` with
     the **daemon-chosen `target`** (configured `target` for Real,
     `config.debug_tool_target` for Debug) + `env_passthrough` / `env_set`
     / `xdg_runtime_dir_base` from config. **CRUX: the daemon picks the
     binary; emit `debug_tool_target` ONLY after the backend allowed a
     `variant = Debug` open. A client sending `variant = Real` can never
     get the debug tool.**
   - `register_wrapper_assets` (`server.rs` 103-143): map key IS the
     bare wrapper name now (drop `logical_name.unwrap_or_else(...)`).
   - `backend_client.rs`: thread `variant` onto the open/finalize request
     structs.
   - `wrapper_resource_channel.rs`: pass the same `variant` into finalize.

2. **1520 — `frontend-exec` single binary.**
   - argv parsing: `uw-<instance> [debug] <wrapper> [args...]`. basename
     identifies the INSTANCE (not the wrapper). Load the single instance
     `DaemonConfig` (no per-wrapper sidecar). `detect_wrapper_config_name_from_executable`
     (260-298) and the `WrapperExecConfig`/`WrapperFile` sidecar structs
     (~320-427) go away / are reworked.
   - Send `Read { wrapper_name, variant, scope }`; receive `ReadOk` (now
     with `target`/env); build the exec for `underwrap-ns` using the
     **daemon-returned** `target` + env (don't read target locally).
   - `ExecBundle` is unchanged structurally (target still reaches ns via
     argv, now sourced from `ReadOk.target`). The `request_secret`
     destructure at `main.rs:443-456` must add the new `ReadOk` fields.

3. **1540 — backend (`backend-backendd`).**
   - `runtime_api/invocations.rs` `resolve_invocation` (39-81): take
     `variant`, apply the matrix via the shared `policy::decide` (now
     `decide(permission, variant, now)`); finalize re-checks variant too.
   - Round-trip `UnsafeDebugUntil` through `admin_api/policy.rs`; OpenAPI
     reflects the new mode + the `variant` request field (regen consumers
     in Phase 3's SPA ticket 1560).
   - Update backend tests/`test_support.rs` for the new request shapes.

4. **integration-tests** — every `WrapperRequest::Read { scope }` and
   `ReadOk { .. }` destructure and the harness `WrapperDef`
   construction (`harness/mod.rs:122`, `tests/daemon.rs:299`, etc.) need
   the new fields. See the consumer map below.

**Bundle exit:** `cargo build`, `cargo test --workspace`, clippy all green.
Then commit the bundle (amend/replace the WIP or stack a follow-up commit
that makes the workspace green). Only after that do Phase 3 / Phase 4.

### Consumer map (from a thorough sweep — fix these callsites)

Removed-field readers: `.socket` → `frontend-underwrapd/startup.rs:54-63`,
`server.rs:194`, `integration-tests/harness/mod.rs:122`,
`tests/daemon.rs:299`. `.group` → `startup.rs:16`. `.logical_name` →
`server.rs:106-109`, `wrapper_flow/read.rs:70-73`.

`WrapperRequest::Read` constructed at `frontend-exec/main.rs:436`,
`integration-tests` harness `mod.rs:262,282`, `tests/daemon.rs:372`,
`tests/store_read.rs:95`, `tests/wrapper_writeback.rs:64`; matched at
`frontend-underwrapd/wrapper_flow/mod.rs:42`.

`ReadOk` built at `frontend-underwrapd/wrapper_flow/read.rs:166-173`;
destructured at `frontend-exec/main.rs:443-456`, `integration-tests`
`tests/mod.rs:121`, `tests/directory_and_status.rs:95-101`,
`tests/store_read.rs:37`, `tests/wrapper_writeback.rs:67-77`.

`xdg_runtime_dir_base` currently originates in the frontend-exec sidecar
(`main.rs:327-415`); post-redesign it comes from `DaemonConfig` and is
relayed in `ReadOk`. The `ExecBundle` field stays.

(Phase 3 then does 1550 NixOS module, 1470 systemd, 1560 SPA, 1440
SO_PEERPIDFD, 1570 VM test. Phase 4 = consolidated docs incl. 1580 +
the deferred doc bits.)

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

## Phase 4 — Consolidated documentation pass

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
