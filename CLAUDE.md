# AGENTS.md

## Project overview

underwrap is a secret brokerage system. A daemon holds secrets and
serves them to setgid wrapper binaries over Unix sockets. Targets run inside
PID namespaces for process isolation. Rust workspace + Nix flake.

**Target platform: NixOS.** The wrapper chain and daemon are deployed
through a NixOS module that wires `security.wrappers`, declarative
user/group management, and file capabilities. `underwrap-backend` is
deployable on any Linux host.

## Threat model and non-goals (read before any security judgment)

- **The caller is the adversary.** underwrap's job is to keep the brokered
  secret — and the private mounts, the libsecret bus, and the injected env —
  hidden from the same-UID caller who launched the wrapper.
- **The target is trusted with the secret.** We hand it the secret. We do
  **not** sandbox or contain the target, and we do not defend against a
  malicious or buggy target leaking the secret (it trivially could). The
  PID/mount/IPC namespaces and the dedicated target UID exist to hide the
  secret *from the caller*, not to confine the target.
- **Baseline is target-runs-as-caller.** Before underwrap, the target ran as
  the caller with full access to its home, session, sockets, and agents.
  underwrap must not be *more* restrictive toward the target than that
  baseline, except where the restriction is required to hide the secret from
  the caller. Incidental loss of caller-session access (e.g.
  `/run/user/<uid>`) is a bug to fix, not a feature to keep.
- **"Does this weaken security?" must name a direction and an asset.** A change
  weakens security only if it exposes a protected asset (secret / mounts / bus
  / env) **to the caller**. Giving the target more access to the caller's own
  resources is not a regression — it restores baseline.

## Release posture

This project is still **greenfield**. It is being shaped for a strong **first
release**, not maintained as a compatibility surface for existing users.

When you work on this codebase, optimize for:

- a clean architecture
- a good-looking first release
- stronger security boundaries
- removing awkward early design choices when a better design is clear

Do **not** preserve backward compatibility just because something already
exists in the tree. It is acceptable to change:

- IPC protocol details
- generated socket layout
- NixOS module structure
- config schema
- naming and terminology

as long as the change improves the design and does **not weaken security**.

Talk to the user explicitly in these terms: this is a greenfield project, the
current goal is a polished first release, and backwards compatibility is not a
constraint unless the user asks for it.

## Commands

```bash
nix develop                                          # dev shell
cargo build                                          # build all
cargo test --workspace                               # 500+ tests
cargo fmt -- --check                                 # format check
cargo clippy --all-targets --workspace -- -D warnings # lint (zero warnings policy)
nix build .#checks.x86_64-linux.vm-test              # NixOS VM test (60+ assertions)
```

## Project structure

```
crates/
  underwrap-protocol/         # IPC framing, config schema, messages, SCM_RIGHTS, policy types
  underwrap-underwrap-backend-store/           # StorageBackend trait used by the backend (PlainBackend, EncryptedBackend)
  underwrap-secretsd-store/ # In-memory Secret Service data model + JSON serialization
  underwrapd/     # Daemon: thin client of the backend; relays wrapper IPC to /v1/wrapper-invocations
  underwrap-exec/           # Wrapper stage 1 (setgid): authenticate, fetch secret, pipe to stage 2
  underwrap-ns/             # Wrapper stage 2 (cap_sys_admin): PID namespace, exec target
  underwrap-secretsd/       # D-Bus Secret Service provider for libsecret mode
  underwrap-keygen/         # `underwrap-keygen`: client mTLS credential / key generation
  underwrap-debug-tool/              # Unsafe debug CLI (env summary / interactive shell / exec, with read-write access to the projected secret); the target the daemon execs for `variant = Debug` (gated by an `UnsafeDebugUntil` permission)
  underwrap-backend/        # Central HTTPS service + admin API + auth + SPA hosting + policy authority
  integration-tests/       # Cargo integration tests (real daemon + backend)
web/                       # Vite + React + shadcn admin SPA (served by underwrap-backend)
nix/
  modules/underwrap.nix         # Multi-instance frontend NixOS module
  modules/underwrap-backend.nix # Retired placeholder (stores are now backend-managed dynamic records)
  tests/node.nix                # VM test node configuration
  tests/test.py                 # VM test script (Python)
  tests/spoof.py                # Socket spoofing test helper
docs/                    # Documentation (backend, security, etc.)
examples/config.yaml     # Annotated example frontend config
tickets/                 # Kanban-style ticket system (see "Ticket system" below)
```

## Ticket system

Work items are tracked as markdown files under `tickets/`, organized as a
simple kanban board. Move a ticket file between folders to reflect its state:

- `tickets/01_todo/`       — queued work
- `tickets/02_inprogress/` — currently being worked on
- `tickets/03_done/`       — completed work

Naming convention:

- epics: `epic_<short_title>.md`
- larger child stories: `NNNN_story_<short_title>.md`
- executable task tickets: `NNNN_task_<short_title>.md`

Ordered ticket numbers use a zero-padded **4-digit** prefix plus underscore,
e.g. `1000_story_...`, `1010_task_...`, `1020_task_...`. New tickets start
from `1000_` and pick a free `NNN0_` block. Lower numbers mean earlier
intended processing order. When breaking down an epic, create or update the
story tickets and give both story and task tickets these ordered numeric
prefixes.

## Key files to understand first

- `crates/underwrap-exec/src/main.rs` — two-stage wrapper entry point;
  authenticates against the daemon, receives env + a scoped resource
  channel fd, then re-execs into `underwrap-ns`
- `crates/underwrap-ns/src/main.rs` — PID namespace + hardening + libsecret mode
- `crates/underwrapd/src/server.rs` — daemon: lazy
  [`BackendClient`] open, wrapper-socket dispatch
- `crates/underwrapd/src/backend_client.rs` — typed mTLS
  client for the three runtime endpoints
- `crates/underwrapd/src/wrapper_flow/` — wrapper socket
  request flow (peer auth → open invocation on backend → relay
  ReadOk + scoped channel)
- `crates/underwrapd/src/wrapper_resource_channel.rs` —
  serves Checkout from the cached blob; turns Checkin / UsageSignal
  into finalize calls
- `crates/underwrap-backend/src/server.rs` — HTTPS service + admin
  API + the three runtime handlers (`/v1/wrappers`,
  `/v1/wrapper-invocations`, `/v1/wrapper-invocations/finalize`) +
  sole policy authority
- `crates/underwrap-backend/src/auth.rs` — Argon2id password hashing + opaque session cookies (tower-sessions `MemoryStore` — sessions do not survive a restart)
- `crates/underwrap-backend/src/web.rs` — embedded SPA hosting via `rust-embed`
- `crates/underwrap-backend/src/users.rs` — flat user records (admin login accounts). Authorization is flat: any authenticated user can perform any admin action; a "last user" guard keeps the collection non-empty. There are no roles or per-store grants.
- `crates/underwrap-protocol/src/policy.rs` — `PolicyDocument`, `WrapperPermission`, `WrapperApprovalMode`
- `crates/underwrap-protocol/src/entities.rs` — `ResourceRecord`, `WrapperResourcePairRecord`, `EntityCreator` (first-class entities with provenance)
- `crates/underwrap-protocol/src/service.rs` — wire types incl.
  `Runtime{WrapperRegister, WrapperInvocationOpen,
  WrapperInvocationFinalize}{Request,Response}`
- `web/src/router.tsx` — TanStack Router setup; the SPA's auth/bootstrap guard lives here
- `nix/modules/underwrap.nix` — frontend NixOS module (generates wrappers, sockets, services)
- `docs/backend.md` — backend data model + API surface
- `docs/security.md` — every security mechanism explained

## Code conventions

- Rust 2024 edition, `anyhow` for errors, `tracing` for logging
- Zero clippy warnings enforced (`-D warnings`)
- `cargo fmt` enforced
- Secret material is NEVER logged — audit events log secret IDs, not values
- All `unsafe` is for well-understood libc FFI only (setgid, setuid, prctl, etc.)
- Return values from privilege-drop syscalls are always checked
- Permanent privilege drops should clear saved IDs as well, so elevated
  wrapper identity cannot be regained later via saved credentials
- No fallback modes — if a security mechanism fails, the wrapper aborts

## Security-critical rules

### Never do

- Never log secret material (values, tokens, credentials)
- The client-supplied wrapper name is a *selector*, never an
  authorization signal. Validate it against config (reject unknown names
  and the reserved `debug` token); the kernel gate is the instance-group
  egid on the 0660 socket, and the backend policy is the authorization
  authority. The daemon — not the wrapper — chooses the target binary.
- Never allow fallback to a less-secure mode (no "degraded" operation)
- Never put secrets in argv, environment variables between stages, or temp files
  during the wrapper handoff (use pipe fd 3)
- Never trust argv[0] (symlinks can set it to anything). The wrapper to
  run comes from the validated name *argument* (`underwrap-<instance>
  [debug] <wrapper>`), not argv[0]; the kernel identity is the instance
  group egid, read race-free via `SO_PEERPIDFD`.
- Never skip return value checks on privilege-drop syscalls
- Never persist secret material outside the central backend's `StorageBackend`
  — no JSON files, sidecar databases, or on-disk caches in the daemon, wrapper,
  or namespace. The daemon caches the backend's opaque blob bytes in process
  memory for the lifetime of the wrapper channel; nothing else stays on disk
  (see `docs/security.md` "Design invariants")
- Never pass the wrapper-resource channel fd (fd 20) to the target process — close it
  before exec'ing the target
- Never remove security test assertions to make tests pass — find and fix the
  root cause instead
- Never put secret values in shell command lines (they leak to systemd journal
  via PAM session logging, process table, and shell history)
- Never let the target inherit the user's full environment — use env_clear()
  with a whitelist. Blocks LD_PRELOAD, LD_LIBRARY_PATH, PATH manipulation.

### Always do

- Gate the wrapper socket on the peer's effective GID == the instance group, read race-free via `SO_PEERPIDFD` (Linux 6.5+; `SO_PEERCRED` + `/proc/<pid>/status` fallback). Then validate the client-supplied wrapper name against config
- Zeroize secret material after use (`zeroize` crate)
- Set `PR_SET_DUMPABLE=0` on wrapper processes that hold secrets
- Set `PR_SET_DUMPABLE=0` in secretsd as first action before reading secrets
  (exec resets dumpable to 1 for normal binaries — see `prctl(2)`)
- Set `RLIMIT_CORE=0` before exec of target
- Use `tempfile::NamedTempFile::persist()` for atomic writes
- Run the full test suite before committing: `cargo test --workspace`
- Run the VM test for NixOS module changes: `nix build .#checks.x86_64-linux.vm-test`
- When synchronizing with dbus-daemon, use `--print-address=<fd>` and read from
  the pipe (NOT socket file polling — the file appears before accept is ready)
- Target process /proc protection depends on Yama LSM. Under
  `hardenTargetHost = true` the NixOS module pins
  `kernel.yama.ptrace_scope = 2` (`mkDefault`). Lowering it to 1 or 0
  weakens the same-UID protection claim; at 0, env-mode secrets are
  exposed to any same-UID process. See security.md.

### Ask first

- Before adding new dependencies (especially native/C libraries)
- Before changing the wrapper binary chain (underwrap-exec → underwrap-ns)
- Before making a change that could weaken security even if it simplifies the design

Protocol, config, naming, and NixOS module structure changes are acceptable in
this greenfield phase when they improve the first-release design. Do not block
on backward compatibility concerns.

## Testing

- Unit tests: in each crate, run with `cargo test -p <crate>`
- Integration tests: `cargo test -p integration-tests` (some skip without cap_sys_admin)
- Admin SPA Vitest: `cd web && npm test`
- Admin SPA Playwright (run inside `nix develop`): `cd web && npm run test:e2e`
- VM test: `nix build .#checks.x86_64-linux.vm-test` — boots a real NixOS VM, tests
  local and remote-style instances against real `underwrap-backend` processes,
  unprivileged user, spoofing resistance, PID namespace isolation, memory
  protection, core dump prevention, permission-driven wrapper locking

### Test file layout (Rust crates)

Use the **sibling-file submodule pattern** rather than inline `mod tests {}`
blocks at the bottom of production files:

```
src/admin_api/csrf.rs           # production code
src/admin_api/csrf/tests.rs     # tests for csrf.rs
```

The production file ends with:

```rust
#[cfg(test)]
mod tests;
```

That declaration makes `csrf/tests.rs` a submodule of `csrf`, so `use super::*;`
inside it reaches every private and `pub(crate)` item — same privacy as an
inline block. Production files stay free of test scaffolding, and the
tests still live next to the code they exercise.

Cross-module helpers (fixture builders, fake stores, request builders) go in
`src/test_support.rs`, declared once at the crate root as
`#[cfg(test)] pub(crate) mod test_support;`. Each `<module>/tests.rs` then
imports what it needs via `use crate::test_support::*;`. Helpers used by
exactly one tests.rs stay in that file.

This pattern is **always** used. No "tiny exception" — even a one-test
module goes in a sibling `tests.rs`. Production files never carry an
inline `#[cfg(test)] mod tests { ... }` block. Aim to keep any single
`tests.rs` under ~600 lines.

**Crate-root exception (`main.rs` / `lib.rs`):** Rust resolves `mod tests;`
in a regular file like `csrf.rs` to `csrf/tests.rs`, but in a crate root
(`main.rs` / `lib.rs`) it resolves to `src/tests.rs` instead — which
breaks the `<stem>/tests.rs` convention and collides when a crate has
both a `main.rs` and a `lib.rs`. To keep the same layout, crate roots
declare the submodule with an explicit `#[path]`:

```rust
// in src/main.rs
#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;
```

The tests then live at `src/main/tests.rs` (or `src/lib/tests.rs` for
`lib.rs`), matching the same `<stem>/tests.rs` shape every other module
uses.

Never add `#[cfg(test)] pub(crate) use ...` re-exports at the crate root just
so an inline test block can reach moved code. If a test needs an item from
another module, import it directly from where it lives.

## Architecture notes

**Two-stage wrapper chain:**
`underwrap-exec` (setgid) → pipe fd 3 → `underwrap-ns` (cap_sys_admin) → target in PID namespace

**Identity model:** one setgid binary and one group, both named
`underwrap-<instance>` (the group is the daemon's primary group), plus
one `0660` socket per instance. The kernel gate is the instance-group egid (read
race-free via `SO_PEERPIDFD`); the client-supplied wrapper name + variant
arrive in the request and the daemon validates the name against config
(rejecting unknown names and the reserved `debug`). The daemon — not the
wrapper — chooses the target (configured target for `Real`,
`debug_tool_target` for `Debug`), emitted only after the backend allows
the variant. The daemon issues `POST /v1/wrapper-invocations {
wrapper_name, resource_name, variant }` over mTLS; the backend re-derives
the principal from the cert and consults the per-store permission
`(client_instance, wrapper, resource)`. The daemon carries no policy
state.

**One resource per wrapper (frontend invariant):** each `WrapperDef` in
the frontend config projects exactly one backend resource. The resource
itself still has all three projection slots (`env`, `files`, `libsecret`);
the wrapper opts into the subset it needs. The backend keeps full
`(wrapper, resource)` granularity in the pair table and the grant key —
collapsing the grant key would let a frontend-side config edit silently
redirect an existing authorization to a different secret, which is the
trust-boundary reason for the asymmetry.

**Central backend:** the frontend daemon connects outbound to
`underwrap-backend` over HTTPS with mTLS. The backend holds secrets, the
policy document, the user collection, and the admin SPA. Users interact
with the backend over its HTTPS admin listener with session-cookie auth
(Argon2id passwords + opaque tower-sessions cookies backed by `MemoryStore`
+ synchronizer-token CSRF, via an HttpOnly `X-CSRF-Token` captured from
admin responses and replayed). Sessions do not survive a backend restart.
Authorization is flat — every authenticated user has full admin access;
there are no roles or per-store grants. The SPA bundle ships embedded
inside the backend binary and is served from the same listener as the
JSON admin API.

**Multi-instance:** `services.underwrap.instances.<name>` — fully isolated users, groups, sockets.

**Wrapper permission model:** the per-store policy document is a flat
list of `WrapperPermission` rows keyed by
`(client_instance, wrapper, resource)`. Each permission carries one of
four modes (`Locked`, `Unlocked`, `UnlockedUntil { until_epoch }`,
`UnsafeDebugUntil { until_epoch }`) and a per-permission `writable: bool`
flag. `UnsafeDebugUntil` is the only mode that admits the `Debug`
variant (the debug tool — an env summary / interactive shell / exec
session with read-write access to the projected secret, so writebacks
from `Debug` are accepted exactly like `Real`, gated only by `writable`);
every other mode denies `Debug` with `not-unsafe-debug`. The shared
`policy::decide(permission, variant, now)` is the single authority for
the (mode × variant) matrix. Read access through an active permission is
unconditional; write access is decided per-permission and re-checked
server-side on every finalize. There is no global write-lock. Per-
invocation approval is out of scope; use OS-level mechanisms (sudo /
polkit) if you need a prompt.

**Runtime API (daemon ↔ backend, mTLS):**

- `POST /v1/wrappers` — register the daemon's `(wrapper_name,
  resource_name)` pairs at startup.
- `POST /v1/wrapper-invocations` — open one invocation, carrying
  `variant` (`real`/`debug`). Backend re-evaluates the policy for the
  variant and returns either `Allow { resource_id, blob_b64, writable }`
  or `Deny { reason }` (e.g. `not-unsafe-debug`).
- `POST /v1/wrapper-invocations/finalize` — close one invocation,
  optionally with a writeback blob. Backend independently re-checks
  writability, re-canonicalizes, and rejects any change to the env
  section (`env-section-immutable`). With `blob_b64 = None` this is
  the usage-signal path that just bumps `last_used_at`.

The runtime client never names a storage key, never identifies
itself by id, never evaluates policy, and never holds a
`StorageBackend`. There is no wire shape that lets a runtime mTLS
client address `control/*` or `<resource>/<slot>/<key>`.

**Libsecret mode:** When `libsecret = true` on a wrapper:

```
PID + mount namespace (created by underwrap-ns)
┌─────────────────────────────────────────────────────┐
│  dbus-daemon (private session bus)                  │
│  underwrap-secretsd (org.freedesktop.secrets)       │
│    └── reads/writes via daemon channel (fd 20)       │
│  target (e.g. gh)                                   │
│    └── DBUS_SESSION_BUS_ADDRESS → private bus        │
└─────────────────────────────────────────────────────┘
```

1. Daemon creates `socketpair()` during GID-authenticated read request
2. One end sent to wrapper via `SCM_RIGHTS`, other kept by daemon as scoped channel
3. `underwrap-ns` creates PID + mount namespace, mounts fresh `/proc`,
   hides real session bus at `/run/user/<uid>/bus`
4. Launches private `dbus-daemon` + `underwrap-secretsd` in namespace
5. secretsd claims `org.freedesktop.secrets` on the private bus
6. Target sees `DBUS_SESSION_BUS_ADDRESS` pointing to private bus
7. All libsecret read/write: secretsd → daemon channel → daemon's central backend
8. fd 20 (channel) is CLOSED before exec'ing the target
9. On namespace exit: kernel SIGKILL kills dbus-daemon + secretsd

See `docs/security.md` "Design invariants" and "Libsecret mode" for security analysis.

## Resource storage and writeback model

Every resource is stored as **one** opaque blob at `resources/<id>`
in the backend's `StorageBackend`. The blob is JCS-canonical (RFC
8785) JSON carrying the resource's `env`, `files`, and `libsecret`
sections (see `docs/backend.md` "Resource blob storage").

The daemon never holds a `StorageBackend`. On wrapper open it gets
the canonical blob bytes from `POST /v1/wrapper-invocations` and
caches them for the lifetime of the channel; on Checkout it serves
those bytes verbatim; on Checkin / UsageSignal it issues
`POST /v1/wrapper-invocations/finalize` (with or without a blob).
Mount paths are a daemon-local concern — `underwrap-ns` derives them
from the wrapper config — and never travel on the wire.

`WrapperResourcePairRecord` carries `last_used_at` (bumped on every
successful open and every successful finalize).

The namespace's bind-mounted directory and tmpfs staging dir are
**always read-write** to the target, regardless of the wrapper
permission. The backend remains the sole authority for write
authorization: when an end-of-run writeback is rejected on a
read-only permission, the wrapper
emits one structured journal event plus one stderr line per affected
resource (suppressible via `UNDERWRAP_SUPPRESS_READONLY_WARNING=1`).
secretsd applies the same warning UX to libsecret writes; the D-Bus
client sees its `SetSecret` as succeeding.

`UNDERWRAP_SUPPRESS_READONLY_WARNING` is honored by `underwrap-ns` and
`underwrap-secretsd`. It is **never** propagated into the target
process's environment.

