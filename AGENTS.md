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

Published and current-state product documentation remains present tense and free of superseded alternatives or planning history.
Designated internal project-memory directories may retain the decisions, evidence, investigations, and future direction defined below.
Git remains historical evidence, but it is not the only owner of active obligations.
Deliberate format or protocol versioning is still valid when it protects the first-release format; greenfield status is not a reason to remove useful version identifiers.

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
  superseded alternatives in published or current-state product documentation.
- Designated internal project-memory directories may retain the records defined
  in the project-memory taxonomy below.
- Prefer one normative explanation and link to it from task-oriented guides.
- Use the terms **runtime client instance**, **resource**, **wrapper/resource
  pair**, **permission row**, **permission mode**, and **writable flag**
  consistently.
- Treat code, generated OpenAPI, and tested Nix options as evidence when prose
  and implementation disagree.

## Agent entry points and reusable skills

`AGENTS.md` is the canonical repository guidance for every coding agent.

`CLAUDE.md` is a compatibility symlink to `AGENTS.md` and must remain a symlink.

`.agents/skills/` is the canonical reusable-skill directory.

`.claude/skills` and `.pi/skills` are compatibility symlinks that point directly to `../.agents/skills`.

Do not maintain vendor-specific copies of shared guidance or skills.

## Git worktree workflow

For every task that mutates this repository, use a dedicated feature branch in a linked worktree.
Create the feature branch from the current local `main` branch unless the task is already running in a suitable linked feature worktree.
Within the Git lifecycle, do not ask whether to use a worktree, which branch should receive completed work, or which integration method to use.
Completed work targets local `main` through an ordinary `--no-ff` merge that creates a merge commit.
After the feature commits and checks are complete, stop before integration and ask the user to approve merging the exact verified feature tip into the exact verified `main` tip.

## Documentation and communication

Keep documentation and conversations concise, clear, accurate, and useful to a newcomer.

Write one sentence per line in newly written prose so diffs remain reviewable.

State assumptions, unknowns, unavailable checks, and user-reported evidence explicitly.

Do not turn an implementation observation into a security claim without direct evidence.

Never quote or copy secret values, credentials, resource blobs, tokens, key material, or secret-bearing fixture contents into plans, notes, reviews, commits, or logs.

## Project-memory document taxonomy

| Location | Purpose | Authority |
|---|---|---|
| `docs/plans/` | Active executable plans with checkboxes, scope, and evidence | Current work owner |
| `docs/plans/archived/` | Fully evidenced completed plans after durable curation | Workflow handoff only |
| `docs/adr/` | Accepted decisions and trade-offs | Maintained decision owner |
| `docs/implementation-notes/` | Current internal mechanisms and empirical findings | Maintained mechanism owner |
| `docs/research/` | Dated investigations, rejected options, and visible retractions | Evidence record |
| `docs/roadmaps/` | Future direction without implementation authority | Direction only |
| `tickets/_review/` | Append-only accepted review reports; existing files are immutable | Historical review evidence |
| `tickets/_handover/` | Immutable accepted review-program state and decisions | Historical migration source |
| `tickets/01_todo/`, `tickets/02_inprogress/`, `tickets/03_done/` | Immutable existing ticket records | Historical work evidence |

Current product behavior remains in `README.md`, `docs/architecture.md`, `docs/security.md`, and the other task-oriented public guides.

## Active planning workflow

### Grounding

Read `docs/plans/README.md`, inspect active plans, and resolve overlapping ownership before creating or revising a plan.

Read the relevant ADRs, implementation notes, research, roadmap entries, security documentation, architecture documentation, testing guidance, and historical review records before planning affected work.

Use the existing `.codegraph` index before structural exploration when it is present.

Revalidate historical review findings against the current tree instead of assuming that old locations or conclusions still apply.

### Plan structure

Create plans at `docs/plans/YYYY-MM-DD_<topic>-plan.md` with a concise lowercase topic.

Every plan contains `## Goal`, `## Plan`, and `## Completion Checklist`.

Use optional sections only when they carry useful context, relationships, architecture, security invariants, non-goals, assumptions, unknowns, risks, or rollback and recovery details.

Write checkbox tasks that name one action, one object, the expected result, and exact acceptance evidence.

### Execution authority

Creating or revising a plan authorizes the plan artifact and its scoped local commit, not implementation.

Implementation requires explicit user authority or execution authority recorded in the plan by the user.

A plan does not authorize a dependency, a product behavior change, a change to the `underwrap-exec → underwrap-ns` chain, or a weaker security boundary without the required focused decision.

Do not create a plan merely to duplicate a completed ticket or to describe this workflow migration.

### Evidence

Check a task only after its stated acceptance evidence passes.

Distinguish direct, simulated, unavailable, and user-reported evidence.

Reopen a checked task when later work invalidates its evidence or changes the relevant boundary.

Do not claim that a unit test proves kernel behavior, zeroization, filesystem durability, target trustworthiness, or operator behavior unless it directly observes that property.

### Curation and archival

Move accepted decisions and trade-offs to `docs/adr/`.

Move current mechanisms and empirical findings to `docs/implementation-notes/`.

Move dated investigations, rejected options, and retractions to `docs/research/`.

Move future direction without authority to `docs/roadmaps/`.

Move user-visible behavior and limitations to the existing public documentation owners.

Archive a plan only after every task and completion check is evidenced and every durable fact and remaining obligation has a current owner.

Never leave the archive as the only owner of durable decisions, mechanisms, measurements, limitations, or open obligations.

## Plan ownership replaces the ticket workflow

The plan workflow replaces ticket materialization and ticket execution for active Underwrap work.

`docs/plans/` owns new non-trivial implementation, migration, release, investigation, and cross-cutting work.

Existing tickets, handovers, and review reports are immutable evidence; `tickets/_review/` is append-only for newly accepted comprehensive-review reports.

Do not create both a plan and a ticket for the same implementation, rewrite historical records, or convert completed tickets into plans.

The completed 2026 review program is recorded in [`docs/plans/archived/2026-08-26_codebase-review-remediation-plan.md`](docs/plans/archived/2026-08-26_codebase-review-remediation-plan.md).

Its D-174 execution authority is exhausted, and the archived plan is historical evidence rather than an active work owner.

Use [the writing-plans skill](.agents/skills/writing-plans/SKILL.md) for the active workflow.

## Skills

| Skill | Use |
|---|---|
| `writing-plans` | Draft, execute, track, curate, and archive active plans |
| `reviewing-code` | Perform ordinary bounded read-only diff, commit, pull-request, or source review |
| `hardening-code-paths` | Confirm and fix edge cases in one specific authorized flow |
| `underwrap-codebase-review` | Perform a whole-repository or explicitly comprehensive subsystem review |
| `underwrap-ticket-workflow` | Read historical ticket records and their prior process; not the default workflow for new work |

Read the selected skill from `.agents/skills/<name>/SKILL.md` before using it.

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
docs/                        current product documentation and internal project memory
docs/plans/                  active executable plans
docs/plans/archived/         completed plan handoffs
docs/adr/                    accepted architecture decisions
docs/implementation-notes/  maintained internal mechanism notes
docs/research/              dated investigations and retractions
docs/roadmaps/              future direction without implementation authority
tickets/                     retained historical review and work records
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
- `crates/underwrap-secretsd/src/main.rs` — one-shot canonical initialization, in-memory state, and private generation publication
- `crates/underwrapd/src/peer_auth.rs` — effective-GID peer authentication
- `crates/underwrapd/src/wrapper_flow/` — wrapper request flow
- `crates/underwrapd/src/wrapper_flow/read.rs` — secret-bearing backend-open decoding, complete typed resource parsing, conditional env-name validation, and original-byte channel caching
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

- Persist underwrap-managed durable brokered resource material only through the backend's selected `StorageBackend`.
  Retained encrypted rotation `backup/` and `stage/` sibling workspaces remain outside the active store path and APIs until trusted-host cleanup.
  Keep frontend resource material in process memory, except for files staged on invocation-private `noswap` tmpfs, and do not add disk caches, sidecar databases, local databases, ordinary temporary files, or alternate durable frontend paths.
  Keep libsecret resource state in process memory; the namespace-private libsecret bus tmpfs carries only runtime socket and helper artifacts without a `noswap` claim.
- Treat runtime identities, backend and CA keys, authentication records, and trusted configuration as separately controlled sensitive domains rather than exceptions to the brokered-resource rule.
- Never place brokered values or other secrets in `envSet`, because it is materialized in Nix-generated configuration; use resource slots instead.
- Never put secret material in argv, between-stage environment variables, shell command lines, or logs.
  The stage handoff uses fd 3.
- Zeroize secret buffers after use where practical.
- Treat changes to the resource schema, backend-open response decoding, env-name validation, secret-bearing logging or cloning, original-byte cache lifetime, Checkout copies, or related buffer ownership as security-sensitive.
- Mandatory review must cover HTTP response decoding in `crates/underwrapd/src/backend_client.rs`, daemon parsing and cache assembly in `crates/underwrapd/src/wrapper_flow/read.rs`, channel handling in `crates/underwrapd/src/wrapper_resource_channel.rs`, the fd-20 protocol codec in `crates/underwrap-protocol/src/message/wrapper_resource.rs`, and the `underwrap-ns` receiver in `crates/underwrap-ns/src/resource_channel.rs` together.
- Resource-schema or writeback changes must also cover backend runtime finalize in `crates/underwrap-backend/src/runtime_api/invocations.rs`.
- Libsecret schema changes must also cover the secretsd publication validator in `crates/underwrap-secretsd/src/publication.rs`, the host publication validator in `crates/underwrap-ns/src/publication.rs`, and their shared strict publication validation in `crates/underwrap-protocol/src/resource_blob.rs`, as applicable.
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
- cross-component runtime behavior: `./crates/integration-tests/run`
  (`x86_64-linux`/`aarch64-linux` only; it supplies the exact current-system
  `web-build`, `packages.<system>.underwrapd/bin/underwrapd`, and
  `packages.<system>.underwrap-backend/bin/underwrap-backend` paths; host
  capability-dependent skips remain separate)
- SPA: `cd web && npm test && npm run build`
- browser flows: `cd web && npm run test:e2e` inside `nix develop`
- NixOS module, identities, namespaces, capabilities, mounts, peer auth:
  `nix build .#checks.x86_64-linux.vm-test`

## Accepted code-review program — completed historical record

The accepted whole-codebase review report at `tickets/_review/codebase_2026-07-18.md`, its handover at `tickets/_handover/codebase_2026-07-18.md`, completed F-001 through F-030 tickets, and the archived F-031 through F-058 plan remain immutable historical evidence.

The user accepted final program completion and archival on 2026-09-01.

D-174's transferred execution authority and the program's severity, transition, cleanup, review, correction, and completion protocol are exhausted and grant no authority for new work.

Read the report, handover, and archived plan before planning work derived from their findings, Opportunities, unresolved items, or evidence.

Revalidate historical conclusions against the current tree and use `docs/plans/` for any newly authorized non-trivial work.

Do not revive or rewrite a historical ticket or the archived plan, and do not infer implementation authority from their approval, completion, continuation, or next-action records.

Accepted lasting decisions from the completed program live in `docs/adr/`, current behavior and limits live in the product documentation and source, and future test obligations live in `docs/roadmaps/`.

Use `.agents/skills/writing-plans/SKILL.md` for new work, `.agents/skills/underwrap-ticket-workflow/SKILL.md` only to interpret historical records, and `.agents/skills/underwrap-codebase-review/SKILL.md` for a future comprehensive review.

## Legacy ticket board

The following directories are retained as historical records and are not the active work board:

- `tickets/01_todo/`
- `tickets/02_inprogress/`
- `tickets/03_done/`

Historical ticket names use a free four-digit `NNN0_` block at or above `1000_`:

- `epic_<name>.md`
- `NNNN_story_<name>.md`
- `NNNN_task_<name>.md`

Do not create a new ticket for work owned by a plan.

Use `.agents/skills/writing-plans/SKILL.md` for new work and `.agents/skills/underwrap-ticket-workflow/SKILL.md` only to interpret existing ticket records.
