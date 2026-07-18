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
