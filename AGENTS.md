# AGENTS.md

## Project overview

underwrap is a secret brokerage system for NixOS. A daemon holds secrets and
serves them to setgid wrapper binaries over Unix sockets. Targets run inside
PID namespaces for process isolation. Rust workspace + Nix flake.

**NixOS only.** The security model depends on `security.wrappers`, declarative
user/group management, and file capabilities. The only non-NixOS component
is `underwrap-backendd` (the remote backend daemon).

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
cargo test --workspace                               # 85+ tests
cargo fmt -- --check                                 # format check
cargo clippy --all-targets --workspace -- -D warnings # lint (zero warnings policy)
nix build .#checks.x86_64-linux.vm-test              # NixOS VM test (21 assertions)
```

## Project structure

```
crates/
  shared-protocol/         # IPC framing, config schema, messages, SCM_RIGHTS, policy types
  shared-store/            # StorageBackend trait: plain, encrypted; mTLS HTTPS service client
  shared-secretsd-store/   # In-memory Secret Service data model + JSON serialization
  frontend-underwrapd/     # Daemon: identity, grant gate, per-call approval, audit
  frontend-exec/           # Wrapper stage 1 (setgid): authenticate, fetch secret, pipe to stage 2
  frontend-ns/             # Wrapper stage 2 (cap_sys_admin): PID namespace, exec target
  frontend-secretsd/       # D-Bus Secret Service provider for libsecret mode
  frontend-debug-tool/     # CLI for debugging wrapper-socket flows
  frontend-keyring-probe/  # Smoke-test binary for the libsecret round-trip
  backend-backendd/        # Central HTTPS service + admin API + auth + SPA hosting
  integration-tests/       # Cargo integration tests (real daemon + backend)
web/                       # Vite + React + shadcn admin SPA (served by backend-backendd)
nix/
  modules/underwrap.nix         # Multi-instance frontend NixOS module
  modules/underwrap-backend.nix # Backend declarative-stores module
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
- larger child stories: `NNN_story_<short_title>.md`
- executable task tickets: `NNN_task_<short_title>.md`

Ordered ticket numbers use a zero-padded 3-digit prefix plus underscore, e.g.
`000_story_...`, `010_task_...`, `020_task_...`. Lower numbers mean earlier
intended processing order. When breaking down an epic, create or update the
story tickets and give both story and task tickets these ordered numeric prefixes.

## Key files to understand first

- `crates/frontend-exec/src/main.rs` — two-stage wrapper entry point with
  per-call approval ssh-agent signing
- `crates/frontend-ns/src/main.rs` — PID namespace + hardening + libsecret mode
- `crates/frontend-underwrapd/src/server.rs` — daemon: identity resolution, wrapper dispatch
- `crates/frontend-underwrapd/src/approval.rs` — pending-challenge store + signature verify
- `crates/frontend-underwrapd/src/wrapper_flow/` — the wrapper socket request flow
- `crates/frontend-underwrapd/src/policy.rs` — grant gate (lookup by `(wrapper, host, user)`)
- `crates/backend-backendd/src/server.rs` — HTTPS service + admin API + stores/users/grants CRUD
- `crates/backend-backendd/src/auth.rs` — Argon2id password hashing + AEAD session cookies
- `crates/backend-backendd/src/web.rs` — embedded SPA hosting via `rust-embed`
- `crates/backend-backendd/src/operators.rs` — operator records + Principal + role/grant model
- `crates/shared-protocol/src/policy.rs` — `PolicyDocument`, `AgentGrant`, `WrapperApprovalMode`
- `crates/shared-store/src/service.rs` — mTLS HTTPS backend client
- `web/src/router.tsx` — TanStack Router setup; the SPA's auth/bootstrap guard lives here
- `nix/modules/underwrap.nix` — frontend NixOS module (generates wrappers, sockets, services)
- `nix/modules/underwrap-backend.nix` — backend NixOS module (declarative stores)
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
- Never trust client-supplied wrapper names — identity comes from kernel egid
- Never allow fallback to a less-secure mode (no "degraded" operation)
- Never put secrets in argv, environment variables between stages, or temp files
  during the wrapper handoff (use pipe fd 3)
- Never trust argv[0] for wrapper identity — derive wrapper name from
  getegid() → group name → strip prefix. argv[0] is user-controlled
  (symlinks can set it to anything).
- Never skip return value checks on privilege-drop syscalls
- Never store secret material in local files outside the StorageBackend — no JSON
  files, no sidecar databases, no caches. ALL secret values flow through the
  daemon's StorageBackend (see `docs/security.md` "Design invariants")
- Never pass the secretsd channel fd (fd 4) to the target process — close it
  before exec'ing the target
- Never remove security test assertions to make tests pass — find and fix the
  root cause instead
- Never put secret values in shell command lines (they leak to systemd journal
  via PAM session logging, process table, and shell history)
- Never let the target inherit the user's full environment — use env_clear()
  with a whitelist. Blocks LD_PRELOAD, LD_LIBRARY_PATH, PATH manipulation.

### Always do

- Verify wrapper identity from the wrapper socket that accepted the connection plus `SO_PEERCRED` effective GID (read from `/proc/pid/status`)
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
- Target process /proc protection depends on Yama LSM (ptrace_scope=1, NixOS
  default). If ptrace_scope=0, env-mode secrets are exposed. See security.md.

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
  local and remote-style instances against real `underwrap-backendd` processes,
  unprivileged user, spoofing resistance, PID namespace isolation, memory
  protection, core dump prevention, grant-driven wrapper locking

## Architecture notes

**Two-stage wrapper chain:**
`underwrap-exec` (setgid) → pipe fd 3 → `underwrap-ns` (cap_sys_admin) → target in PID namespace

**Identity model:** wrapper socket selects expected wrapper + setgid group → `SO_PEERCRED` effective GID → daemon confirms the match; daemon resolves the GID-validated peer UID to a username via `getpwuid` and consults the per-store grant `(wrapper, host, user)`.

**Central backend:** the frontend daemon connects outbound to
`underwrap-backendd` over HTTPS with mTLS. The backend holds secrets, the
policy document, the operator collection, and the admin SPA. Operators
interact with the backend over its HTTPS admin listener with session-cookie
auth (Argon2id passwords + AEAD session cookies + double-submit CSRF). The
SPA bundle ships embedded inside the backend binary and is served from
the same listener as the JSON admin API.

**Multi-instance:** `services.underwrap.instances.<name>` — fully isolated users, groups, sockets.

**Grant model:** the per-store policy document is a flat list of
`AgentGrant` rows keyed by `(wrapper, host, user)` plus a single
global `writes_enabled` toggle. Each grant carries one of four modes
(`Locked`, `Unlocked`, `UnlockedUntil { until_epoch }`,
`PerCallApproval { approval_key }`). Read access through an active
grant is unconditional; write access (admin + wrapper) gates on the
global `writes_enabled` flag.

**Libsecret mode:** When `libsecret = true` on a wrapper:

```
PID + mount namespace (created by underwrap-ns)
┌─────────────────────────────────────────────────────┐
│  dbus-daemon (private session bus)                  │
│  underwrap-secretsd (org.freedesktop.secrets)       │
│    └── reads/writes via daemon channel (fd 4)        │
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
8. fd 4 (channel) is CLOSED before exec'ing the target
9. On namespace exit: kernel SIGKILL kills dbus-daemon + secretsd

See `docs/security.md` "Design invariants" and "Secretsd channel" for security analysis.

**Per-call approval** (when the grant for `(wrapper, host, user)` is in `PerCallApproval` mode):

1. underwrap-exec connects and sends `WrapperRequest::Read`
2. underwrapd consults the grant and responds with
   `WrapperResponse::ApprovalRequired { challenge_id, nonce, approval_key }`
3. underwrap-exec signs the nonce via `SSH_AUTH_SOCK` and sends
   `WrapperRequest::ApprovalProof { challenge_id, signature, public_key_blob }`
4. underwrapd verifies the signature against the grant's `approval_key`,
   consumes the nonce, and re-authorizes the read with the mode gate
   bypassed
5. The usual `ReadOk` / scoped-channel flow continues

## Completed work

The libsecret mode is implemented. See the architecture notes above and
`docs/security.md` for the full design and security analysis.

## Planned work

- None currently. See git log for recent libsecret mode implementation.
