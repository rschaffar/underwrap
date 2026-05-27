# Epic: Single per-instance wrapper + runtime debug mode (2026-05)

## Background

Today every wrapper is its own setgid binary, its own Unix group, and
its own daemon socket. The daemon derives wrapper identity from
*which socket* a connection arrived on plus the `SO_PEERCRED` egid,
and cross-checks the two. The debug tool is an opt-in
(`addUnsafeDebugTool`) that bakes a *second* per-wrapper binary +
socket into the host at deploy time — far too much friction to be
the real debugging workflow.

We dug into the actual enforcement and found the premise behind the
per-wrapper design is weaker than the docs imply:

- The setgid wrapper binaries are **world-executable**; `testuser`
  in the VM test invokes wrappers without being a member of any
  wrapper group (`nix/tests/test.py`, `node.nix:326`
  `allowedUsers = [ "testuser" ]` only drives **PATH symlinks**,
  not group membership).
- So **there is no per-user access control**. Any host user can
  already invoke any wrapper. The per-wrapper group is purely a
  *wrapper-identity / anti-spoofing* device, and the per-instance
  policy (keyed by `client_instance`, not by user) is the only
  authorization gate.
- The two-factor socket+egid identity check therefore exists *only*
  to disambiguate which-wrapper among many — a cost the multi-socket
  design imposes on itself, not an independent security property.

The real, kernel-enforced boundary is narrower and survives a
collapse: **raw secret access requires going through the
root-installed setgid binary** (a random process has its login
egid, not the instance group, so it cannot open the `0660`
group-owned daemon socket), and **the target binary is fixed by
config, not chosen by the user** (so the secret only ever flows
into the configured target with full hardening — `env_clear`,
`PR_SET_DUMPABLE=0`, namespace, `RLIMIT_CORE=0`).

This epic collapses the per-wrapper multiplicity into **one setgid
binary, one group, one socket per instance**, and folds the debug
tool in as a runtime `variant` rather than a separate binary.

## Target model

- One setgid binary per instance: `uw-<instance>`, setgid to a
  single group `underwrap-<instance>`, talking to a single socket.
  A **default** instance also installs a bare `uw` symlink.
- Normal call: `uw-<instance> <wrapper-name> <args...>`.
- Debug call: `uw-<instance> debug <wrapper-name> [debug-params...]`.
  `debug` is a **reserved** first token; a wrapper may not be named
  `debug`.
- The wrapper sends the (now client-supplied) `wrapper_name` plus a
  `variant: Real | Debug` over IPC. The daemon **validates the name
  against its config** (rejecting unknown names and `debug`), asks
  the backend with the variant, and — critically — **returns the
  target to exec**: the configured target for `Real`, the debug-tool
  for `Debug`. The user never chooses the target.
- New policy mode `UnsafeDebugUntil { until_epoch }` is the **only**
  mode that allows `variant = Debug`. Every other mode denies it.

### Why the daemon must control the target

If `frontend-exec` picked the debug-tool target from the user-typed
`debug` keyword, a user could send `variant = Real` (passing a normal
`Unlocked` policy check) while locally exec'ing the debug tool —
dumping the secret past `PR_SET_DUMPABLE=0` with no
`UnsafeDebugUntil` grant. So target selection moves onto the IPC
wire, decided by the daemon (the side that talks to the policy
authority). This also lets the per-wrapper `frontend-exec` sidecar
configs collapse into the single daemon-held config.

### Backend decision matrix (unchanged from prior thinking)

| mode                           | `Real`           | `Debug`                           |
|--------------------------------|------------------|-----------------------------------|
| `Locked`                       | Deny             | Deny                              |
| `Unlocked`                     | Allow            | Deny `"not in unsafe-debug mode"` |
| `UnlockedUntil` not expired    | Allow            | Deny `"not in unsafe-debug mode"` |
| `UnlockedUntil` expired        | Deny `"expired"` | Deny `"expired"`                  |
| `UnsafeDebugUntil` not expired | Allow            | Allow                             |
| `UnsafeDebugUntil` expired     | Deny `"expired"` | Deny `"expired"`                  |

`writable` is re-checked at finalize as today.

## Trust-model change (must be documented)

`CLAUDE.md` currently says "Never trust client-supplied wrapper
names — identity comes from kernel egid." This epic **deliberately
changes that rule** for the wrapper-name: the name becomes a
client-supplied selector among configs, gated by (a) the instance
group egid on the socket and (b) daemon-side validation against
config, with the policy as the authorization authority and the
daemon-chosen target as the secret-routing guarantee. `docs/security.md`
and `CLAUDE.md` need real surgery, not just an added section. This
is sound (see Background) but it is the central thing a reviewer
must sign off on.

## Tickets

1. **Shared-protocol: instance config + IPC identity**
   Instance-level `socket` / `group` / `ns_binary` /
   `debug_tool_target` on `DaemonConfig`; `WrapperDef` drops
   `socket`/`group`, gains `env_passthrough`/`env_set` so the daemon
   becomes the single config source; `WrapperRequest::Read` carries
   `wrapper_name` + `variant`; `WrapperResponse::ReadOk` carries the
   daemon-chosen `target` + env; `WrapperVariant` enum; reserved
   `debug` name validation.
   → `1500_task_protocol_instance_config_and_ipc`

2. **Shared-protocol: UnsafeDebugUntil policy + matrix**
   `WrapperApprovalMode::UnsafeDebugUntil`, `variant` on the runtime
   open/finalize requests, `policy::decide` matrix. Tests.
   → `1510_task_policy_unsafe_debug_mode`

3. **frontend-exec: single binary + argv dispatch**
   Parse `[debug] <wrapper-name> [args...]`; basename identifies the
   *instance*, not the wrapper; load the instance config; send
   `Read { wrapper_name, variant }`; exec `ns` with the
   **daemon-returned** target + env.
   → `1520_task_frontend_exec_single_binary`

4. **Daemon: single socket, name validation, variant relay, target authority**
   Bind one socket per instance with the instance gid; intake checks
   egid == instance gid, reads client `wrapper_name` + `variant`,
   validates the name against config (reject unknown / `debug`),
   relays to backend with `variant`, and builds `ReadOk` with the
   target (configured vs `debug_tool_target`) + env + debug metadata.
   → `1530_task_daemon_single_socket_authority`

5. **Backend: variant-aware evaluation + admin API**
   `resolve_invocation` + finalize apply the matrix; admin API
   round-trips `UnsafeDebugUntil`. Tests.
   → `1540_task_backend_variant_evaluation`

6. **NixOS module: one wrapper per instance**
   Generate a single `uw-<instance>` setgid binary, one
   `underwrap-<instance>` group, one socket, one daemon config; a
   default-instance `uw` symlink; drop per-wrapper
   groups/binaries/sockets, the per-wrapper sidecar yamls, and
   `addUnsafeDebugTool`.
   → `1550_task_nixos_single_instance_wrapper`

7. **Admin SPA: UnsafeDebugUntil mode + confirm dialog**
   Radio option, `ConfirmDangerDialog` on first selection, shorter
   default TTL, warning badge. Tests.
   → `1560_task_admin_spa_unsafe_debug_mode`

8. **VM test: single-binary + debug subcommand**
   Rework assertions for `uw-<inst> <wrapper>` and
   `uw-<inst> debug <wrapper>`; debug denied unless `UnsafeDebugUntil`;
   spoofing resistance against the single instance socket; unknown /
   `debug`-named wrapper rejected.
   → `1570_task_vm_test_single_binary`

9. **Docs + CLAUDE.md rewrite**
   Rewrite the identity section of `docs/security.md`, update
   `docs/backend.md`, `examples/config.yaml`, and the
   security-critical rules in `CLAUDE.md` to the new trust model.
   → `1580_task_docs_trust_model_rewrite`

## Order of work

`1500` blocks everything. `1510` depends on `1500` (uses
`WrapperVariant`). `1520` and `1530` depend on `1500` (+`1510` for
variant). `1540` depends on `1500`+`1510`. `1550` depends on `1500`
(config shape). `1560` depends on `1540` (OpenAPI regen). `1570`
depends on `1520`+`1530`+`1540`+`1550`. `1580` lands last.

## Landing strategy (important)

Tickets `1500`–`1540` are **not independently shippable**. They
change foundational `shared-protocol` types (`WrapperDef`,
`DaemonConfig`, `WrapperRequest`/`WrapperResponse`,
`WrapperApprovalMode`) that `frontend-exec`, `frontend-underwrapd`,
`backend-backendd`, and `integration-tests` all consume. The moment
`1500`/`1510` land, every consumer fails to compile until `1520`/
`1530`/`1540` land too. Adding an enum variant to
`WrapperApprovalMode` likewise breaks exhaustive matches across the
backend and daemon.

So treat `1500`–`1540` as **one coordinated cutover** (one branch,
landed together) — consistent with this project's no-legacy-cutover
posture (replace in one shot, never ship a half-migrated tree). The
per-ticket split below is a work-breakdown for authoring and review,
**not** a sequence of independently green commits. The
`cargo build`/`cargo test --workspace` green bar is a gate on the
*bundle*, not on each ticket in isolation; each ticket's own
"Acceptance" focuses on its specific behavior and its own added
tests.

`1550` (Nix), `1560` (web), `1570` (VM test), and `1580` (docs) are
separate languages/surfaces and *can* land as their own commits once
the Rust bundle is in.

## Acceptance

Epic closes when all nine child tickets are in `tickets/03_done/`,
and:

- `cargo fmt -- --check`, `cargo clippy --all-targets --workspace -- -D warnings`,
  `cargo test --workspace` all clean.
- `cd web && npm test && npm run test:e2e` clean.
- `nix build .#checks.x86_64-linux.vm-test` green with the reworked
  single-binary + debug-subcommand assertions.
- Manual: `uw-<inst> gh-authorized ...` runs normally;
  `uw-<inst> debug gh-authorized` is denied until an operator sets
  `UnsafeDebugUntil` in the SPA (with the confirm modal), then
  prints the projected env; flipping back to `Locked` denies it
  again. A user-supplied `variant = Real` request can never cause
  the debug tool to be exec'd (target is daemon-chosen).
