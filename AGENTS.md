# AGENTS.md

## Project purpose

underwrap brokers secrets to local CLI targets without giving the caller the
reusable secret directly. The supported frontend is NixOS:

- `underwrap-backend` stores resources and policy and serves the admin SPA.
- one `underwrapd` daemon per frontend instance connects to the backend over
  mTLS and brokers local wrapper requests.
- `underwrap-exec` is the setgid entry stage.
- `underwrap-ns` creates the target namespaces, switches to a dedicated target
  UID, projects the resource, and executes the daemon-chosen target.

The backend can run on any Linux host. The privileged frontend wrapper chain is
supported only through the NixOS module.

## Release posture

This is a **greenfield project being shaped for a polished first release**.
There is no deployed compatibility surface unless the user explicitly says
otherwise.

Prefer the clean design now:

- rename unclear concepts;
- simplify protocol and configuration shapes;
- remove superseded paths instead of retaining compatibility branches;
- strengthen security boundaries;
- keep documentation in present tense, as if the first release had always used
  the resulting design.

Planning history belongs in tickets and Git, not in current-state source
comments or published documentation. Deliberate format or protocol versioning
is still valid when it protects the first-release format; greenfield status is
not a reason to remove useful version identifiers.

## Threat model

Read `docs/security.md` before making security judgments.

- **The caller is untrusted with respect to the brokered secret.** This includes
  other non-root processes owned by the caller.
- **The configured target is trusted with the projection it receives.** A target
  can print, persist, or transmit its own secret; underwrap is not a behavioral
  sandbox. Treat target-supplied protocol input (especially D-Bus input to
  `underwrap-secretsd`) as untrusted for robustness and storage integrity.
- **The backend, root-owned NixOS configuration, Linux kernel, and host root are
  trusted.** Root or kernel compromise is out of scope.
- **Dedicated target UIDs are the primary `/proc` confidentiality boundary.**
  Every `(instance, wrapper)` pair has its own target UID. Never fall back to
  running the target as the caller. Yama `ptrace_scope=2` adds protection for
  ATTACH-mode access; it does not protect READ-mode files such as
  `/proc/<pid>/environ`.
- **Local users are not backend principals.** `allowedUsers` installs convenient
  PATH shims only. The setgid instance entry point is system-wide, and backend
  permissions identify the mTLS-authenticated runtime client instance, not the
  local Unix user. Use an OS-level gate such as sudo or polkit when local-user
  authorization or a prompt is required.

When discussing whether a change weakens security, name the protected asset,
the attacker, and the direction of the boundary.

## Documentation map

- `README.md` — product overview and maturity
- `docs/index.md` — documentation navigation
- `docs/architecture.md` — components and request/data flow
- `docs/security.md` — threat model, guarantees, mechanisms, and limitations
- `docs/backend.md` — backend data model and HTTP API
- `docs/configuration.md` — NixOS and backend configuration
- `docs/testing.md` — validation commands and coverage
- `docs/secretsd-security.md` — target-facing Secret Service input boundary

Documentation rules:

- Describe only the intended first-release design.
- Do not keep historical plans, migration notes, ticket references, or
  superseded alternatives in `docs/`.
- Prefer one normative explanation and link to it from task-oriented guides.
- Use the terms **runtime client instance**, **resource**, **wrapper/resource
  pair**, **permission row**, **permission mode**, and **writable flag**
  consistently.
- Treat code, generated OpenAPI, and tested Nix options as evidence when prose
  and implementation disagree.

## Workspace

```text
crates/
  underwrap-protocol/        shared config, IPC, wire types, policy, resources
  underwrap-backend-store/   plain/encrypted StorageBackend implementations
  underwrap-secretsd-store/  in-memory Secret Service data model
  underwrapd/                frontend daemon and backend runtime client
  underwrap-exec/            setgid wrapper stage
  underwrap-ns/              namespace, target UID, projection, and exec stage
  underwrap-secretsd/        private Secret Service provider
  underwrap-debug-tool/      unsafe incident-debug target
  underwrap-keygen/          runtime client credential generator
  underwrap-backend/         HTTPS runtime/admin service and embedded SPA
  integration-tests/         real daemon/backend integration tests
web/                         Vite + React admin SPA
nix/modules/underwrap.nix    NixOS frontend module
nix/tests/                   end-to-end NixOS VM test
examples/                    annotated deployment/configuration examples
docs/                        current first-release documentation
tickets/                     project work board
```

## Key implementation files

- `crates/underwrap-exec/src/main.rs` — wrapper invocation, peer request,
  privilege drop, fd handoff
- `crates/underwrap-ns/src/main.rs` — namespace lifecycle and target execution
- `crates/underwrap-ns/src/handoff.rs` — fixed-fd daemon peer authentication
- `crates/underwrap-ns/src/target_uid.rs` — root-owned target-UID authority and
  identity switch
- `crates/underwrap-ns/src/idmap.rs` — explicit caller-directory access
- `crates/underwrap-ns/src/writeback.rs` — canonical change detection and
  finalize dispatch
- `crates/underwrap-secretsd/src/main.rs` — one-shot init and tmpfs state model
- `crates/underwrapd/src/peer_auth.rs` — effective-GID peer authentication
- `crates/underwrapd/src/wrapper_flow/` — wrapper request flow
- `crates/underwrapd/src/wrapper_resource_channel.rs` — scoped resource channel
- `crates/underwrapd/src/backend_client.rs` — three-endpoint mTLS client
- `crates/underwrap-backend/src/runtime_api/` — runtime policy enforcement
- `crates/underwrap-backend/src/admin_api/` — authenticated operator API
- `crates/underwrap-backend/src/auth.rs` — password/session authentication
- `crates/underwrap-protocol/src/policy.rs` — permission mode × variant matrix
- `crates/underwrap-protocol/src/service/runtime.rs` — runtime wire contract
- `crates/underwrap-protocol/src/resource_blob.rs` — canonical resource format
- `web/src/router.tsx` — SPA auth/bootstrap guard
- `nix/modules/underwrap.nix` — users, wrappers, capabilities, services, config

## Commands

```bash
nix develop
cargo build
cargo test --workspace
cargo fmt -- --check
cargo clippy --all-targets --workspace -- -D warnings
cd web && npm test && npm run build
nix build .#checks.x86_64-linux.vm-test
nix flake check
```

Use focused checks while iterating. Before committing, run every check relevant
to the changed boundary. NixOS module or wrapper-chain changes require the VM
test.

## Engineering conventions

- Rust 2024, `anyhow` for contextual errors, `tracing` for structured logs.
- Zero clippy warnings and rustfmt-clean code.
- Secret values, tokens, credentials, resource blobs, and key material are
  never logged. Log identifiers and decision metadata only.
- `unsafe` is limited to reviewed syscall/libc FFI and raw-fd ownership
  transitions. Every unsafe block must make its pointer, lifetime, fd-ownership,
  or syscall invariant clear.
- Check every privilege-changing syscall return value. Permanent drops clear
  real, effective, and saved IDs.
- Security mechanisms fail closed. A weaker mode is acceptable only when it is
  an explicit operator-selected posture with separately documented claims; a
  runtime failure must not silently downgrade the active posture.
- Use `tempfile::NamedTempFile::persist()` or an equivalent atomic replacement
  for persistent writes.
- Ask before adding dependencies, changing the `underwrap-exec → underwrap-ns`
  binary chain, or weakening a security boundary.

## Security-critical rules

### Secret persistence and transport

- Persist secret material only through the backend's `StorageBackend`.
  Namespace-private tmpfs staging for files/libsecret is the documented
  ephemeral exception; do not add disk caches, sidecar databases, or ordinary
  temp files on the frontend.
- Never put secret material in argv, between-stage environment variables, shell
  command lines, or logs. The stage handoff uses fd 3.
- Zeroize secret buffers after use where practical.
- Keep the runtime API at wrapper/resource vocabulary. A runtime client must
  never be able to name `control/*` or a storage key.

### Local wrapper identity

- The client-supplied wrapper name is a selector, never proof of identity or
  authorization.
- Gate the `0660` instance socket on peer effective GID matching the instance
  group. In the default hardened posture, resolve it race-free through
  `SO_PEERPIDFD` on Linux 6.5+.
- The `SO_PEERCRED + /proc` path belongs only to the explicitly weakened
  non-hardened posture. Failure while using the pidfd path must reject the peer,
  not fall back.
- Reject unknown wrapper names and the reserved `debug` token.
- Never trust `argv[0]`. The validated wrapper argument selects config, and the
  daemon chooses the target after backend authorization.

### Target execution

- Resolve the target UID/GID only from the root-owned
  `/etc/underwrap/target-uids` table keyed by `(instance, wrapper)`. Do not put a
  target UID/GID in the daemon response, IPC bundle, argv, or caller-controlled
  environment.
- A missing selector, table, row, user, or group is fatal. Never execute as the
  caller or root as a fallback.
- Clear supplementary groups, permanently switch IDs, set
  `PR_SET_NO_NEW_PRIVS`, clear capabilities, and verify the drop before exec.
- Set `PR_SET_DUMPABLE=0` before processes read secrets. `underwrap-secretsd`
  does this as its first secret-handling action.
- Set `RLIMIT_CORE=0` before the target exec.
- Launch the target with `env_clear()` and the documented allowlist. Never
  propagate `LD_*` variables or wrapper-control variables.
- `underwrap-ns` is a world-executable capability wrapper. Before reading fd 3,
  trusting selectors or target argv, or doing privileged setup, require fixed
  fd 20 to be an `AF_UNIX` stream socket whose `SO_PEERCRED` UID/GID matches the
  root-provisioned `underwrap-<instance>` daemon account. Never accept a
  bundle-selected channel fd or fall back after handoff-auth failure.
- Close or mark close-on-exec every fd above stderr before the target exec. In
  particular, the target must never inherit fd 20, the resource channel.
- Synchronize dbus-daemon readiness with `--print-address=<fd>` and a pipe; a
  socket path appearing does not mean the daemon accepts connections yet.

### Backend policy and writeback

- The backend is the policy authority. `underwrapd` forwards decisions and does
  not carry a policy document.
- Re-evaluate the permission mode on open and finalize. Re-check the writable
  flag on every submitted writeback.
- Treat env as operator-owned. Finalize rejects a submitted env section that
  differs from the snapshot it loads, but the current whole-blob
  load/compare/save path is not transactional: concurrent mutations are
  last-writer-wins. Do not claim compare-and-swap semantics or weaken this
  boundary when changing storage coordination.
- Files and libsecret staging are writable to the target. Read-only means
  changes do not persist; do not make a read-only mount fallback that changes
  target behavior.
- Never remove or weaken security assertions merely to make a test pass.

## Test layout

Rust tests use sibling-file submodules; production files do not contain inline
`mod tests { ... }` blocks.

```text
src/admin_api/csrf.rs
src/admin_api/csrf/tests.rs
```

A regular production file ends with:

```rust
#[cfg(test)]
mod tests;
```

Crate roots use an explicit path so `main.rs` and `lib.rs` do not collide:

```rust
#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;
```

Put helpers shared across test modules in `src/test_support.rs`, declared once
from the crate root under `#[cfg(test)]`. Keep helpers used by one module in that
module's `tests.rs`. Do not add crate-root test-only re-exports just to bypass
module privacy. Aim to keep each `tests.rs` below roughly 600 lines.

Validation by area:

- crate logic: `cargo test -p <crate>`
- cross-component runtime behavior: `cargo test -p integration-tests`
- SPA: `cd web && npm test && npm run build`
- browser flows: `cd web && npm run test:e2e` inside `nix develop`
- NixOS module, identities, namespaces, capabilities, mounts, peer auth:
  `nix build .#checks.x86_64-linux.vm-test`

## Accepted code-review program workflow

For the accepted whole-codebase review in
`tickets/_review/codebase_2026-07-18.md`, maintain the durable program state in
`tickets/_handover/codebase_2026-07-18.md`. Read the report and handover before
planning or resuming any review-derived work. Record ticket mappings, approvals,
decisions, ordered cleanup and fix commits, outcomes, validation, dependencies,
exceptions, affected findings, and accumulated lessons there so later work is
informed by earlier results.

The parent/controller remains the sole agent/controller decision-maker, subject
to the user approvals and conditions below, and controls all repository writes.
Process findings by severity in the order critical, high, medium, low.
The program starts directly in the critical band; there is no separate initial
band gate. Before transitioning critical → high, high → medium, or medium → low,
the controller must use `ask_user` with comment entry enabled and record the
result. A lower-severity prerequisite may be advanced only when necessary and
must be recorded as an exception in the handover.

Create exactly one ticket file at a time—normally one task-only ticket, with no
simultaneous parent/child hierarchy—then stop and ask the user to approve that
specific ticket with comment entry enabled before implementation. Do not infer
implementation approval from report acceptance, band-transition approval, or
ticket creation. Rejection or requested changes permit only revision and
re-presentation of that same ticket, or stopping. The first committed work after
approval must also durably move the ticket to in-progress and update the
handover.

Before an approved ticket's actual finding fix, run a simplification-cleanup
phase. Explicitly incentivize workers to find concrete repository-wide as well
as ticket-local behavior-preserving simplifications. Do not perform a one-time
broad refactor ahead of the review; keep sequencing fix-led by severity, with
cleanup immediately before each approved ticket's fix. Accept only cleanup that
has a clear net reduction in complexity, duplication, or dead structure, is
independently reversible, and does not chase optional formatting or renaming.
Cleanup is always behavior-preserving: user approval does not convert a
behavior-changing or boundary-weakening change into cleanup; handle that only as
separately approved fix or Opportunity scope. Cleanup must not add a dependency,
change the `underwrap-exec → underwrap-ns` chain, expose secrets, or absorb an
unresolved decision without `ask_user`.

Every accepted simplification is its own coherent commit before any actual fix
commit for that ticket. Give each cleanup its own fresh independent review,
separate fix pass when needed, focused and boundary-required validation,
controller sanity/scope/diff check, and handover update. A ticket may have
multiple cleanup commits and multiple fix commits. Apply the same independent
review/fix and controller checks to each coherent fix unit. The final fix commit
moves the ticket to `tickets/03_done/` and records final outcome and validation
in the handover. The ticket registry records the ordered implementation sequence
and labels each implementation commit `cleanup` or `fix`.

After the final fix commit, the controller presents the completed outcome,
ordered commits, validation, reviewer result, controller sanity check, handover
decisions, and effects on later findings, then uses `ask_user` with comment entry
enabled for completion review. If changes are requested, return the same ticket
to in-progress for another coherent fix/review/validation/commit cycle; do not
prepare another ticket. Record completion-review and severity-transition
responses in exact-allowlist handover-only `control` commits outside the ticket's
cleanup/fix sequence. Do not derive or prepare the next ticket until the user
explicitly accepts the completed ticket and authorizes continuation. If
continuation would cross a severity band, the separate transition gate follows
this completion-review gate.

If cleanup changes or resolves evidence for another F-id, immediately record the
affected finding and cleanup commit in the handover. When that finding is
reached, revalidate it and link the completed work rather than duplicate a
ticket. Opportunities and U-items still require their own explicit approval.
Use one implementation writer, separate fresh reviewer subagent(s), and a
separate fix-worker subagent for accepted review fixes. The controller
synthesizes reviews and independently checks scope, the actual diff, sanity,
security invariants, and required validation.

On resume, inspect Git state, read the report and handover, locate the sole
current ticket and its recorded approval, then continue from the next incomplete
protocol step; never recreate state from memory or assume approval. Use
`.pi/skills/underwrap-ticket-workflow/SKILL.md` for ticket mechanics, preflight,
content, validation, moves, and exact staging. For this named program only,
materialization may write exactly the single new todo ticket and this handover;
the report, source, tests, configuration, and published docs remain read-only.
An approval-pending todo ticket may remain untracked; after approval, stage its
old todo path only if tracked, otherwise stage the new in-progress path. This
section also supersedes the skill's single-commit per-task loop with the approved
cleanup-first multi-commit and handover-only control-commit protocol.

## Ticket board

Tickets move through:

- `tickets/01_todo/`
- `tickets/02_inprogress/`
- `tickets/03_done/`

Names use a free four-digit `NNN0_` block at or above `1000_`:

- `epic_<name>.md`
- `NNNN_story_<name>.md`
- `NNNN_task_<name>.md`

Lower numbers indicate intended order. Follow
`.pi/skills/underwrap-ticket-workflow/SKILL.md` when the user asks to execute a
ticket.
