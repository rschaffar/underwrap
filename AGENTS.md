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
  underwrap-protocol/    # Shared: IPC framing, config schema, messages, SCM_RIGHTS, policy types
  underwrap-store/       # StorageBackend trait: plain, encrypted; mTLS HTTPS service client
  underwrapd/            # Frontend daemon: identity, per-call approval, audit, secretsd channels
  underwrap-backendd/    # Central HTTPS service + admin API; storage; expiry sweeper
  underwrap-admin-ui/    # Operator web UI in front of the admin API
  underwrap-exec/        # Wrapper stage 1 (setgid): authenticate, fetch secret, pipe to stage 2
  underwrap-ns/          # Wrapper stage 2 (cap_sys_admin): PID namespace, exec target
  underwrap-secretsd/    # D-Bus Secret Service provider for libsecret mode
  underwrap-secretsd-store/ # In-memory Secret Service data model + JSON serialization
  integration-tests/     # Cargo integration tests
nix/
  modules/underwrap.nix  # Multi-instance NixOS module
  tests/node.nix         # VM test node configuration
  tests/test.py          # VM test script (Python)
  tests/spoof.py         # Socket spoofing test helper
docs/                    # Documentation (backend, admin UI, security, etc.)
examples/config.yaml     # Annotated example frontend config
```

## Key files to understand first

- `crates/underwrap-exec/src/main.rs` — two-stage wrapper entry point with
  per-call approval ssh-agent signing
- `crates/underwrap-ns/src/main.rs` — PID namespace + hardening + libsecret mode
- `crates/underwrapd/src/server.rs` — daemon: identity resolution, wrapper dispatch
- `crates/underwrapd/src/approval.rs` — pending-challenge store + signature verify
- `crates/underwrapd/src/wrapper_flow.rs` — the wrapper socket request flow
- `crates/underwrap-backendd/src/server.rs` — HTTPS service + admin API + expiry sweeper
- `crates/underwrap-admin-ui/src/server.rs` — axum routes for the operator UI
- `crates/underwrap-protocol/src/policy.rs` — PolicyDocument, WrapperApprovalMode
- `crates/underwrap-store/src/service.rs` — mTLS HTTPS backend client
- `nix/modules/underwrap.nix` — NixOS module (generates everything)
- `docs/backend.md` — the backend's data model and API surface
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
- Admin UI end-to-end: `cargo test -p underwrap-admin-ui --test end_to_end`
- VM test: `nix build .#checks.x86_64-linux.vm-test` — boots a real NixOS VM, tests
  local and remote-style instances against real `underwrap-backendd` processes,
  unprivileged user, spoofing resistance, PID namespace isolation, memory
  protection, core dump prevention, policy-driven wrapper locking

## Architecture notes

**Two-stage wrapper chain:**
`underwrap-exec` (setgid) → pipe fd 3 → `underwrap-ns` (cap_sys_admin) → target in PID namespace

**Identity model:** wrapper socket selects expected wrapper + setgid group → `SO_PEERCRED` effective GID → daemon confirms the match

**Central backend:** the frontend daemon connects outbound to
`underwrap-backendd` over HTTPS with mTLS. The backend holds secrets + the
policy document. Operators manage the backend through the web-based
`underwrap-admin-ui` service using bearer tokens on the admin API.

**Multi-instance:** `services.underwrap.instances.<name>` — fully isolated users, groups, sockets.

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

**Per-call approval** (when a wrapper is in that backend mode):

1. underwrap-exec connects and sends `WrapperRequest::Read`
2. underwrapd evaluates policy and responds with
   `WrapperResponse::ApprovalRequired { challenge_id, nonce, approval_key }`
3. underwrap-exec signs the nonce via `SSH_AUTH_SOCK` and sends
   `WrapperRequest::ApprovalProof { challenge_id, signature, public_key_blob }`
4. underwrapd verifies the signature against the policy-stored key,
   consumes the nonce, and re-authorizes the read with the mode gate
   bypassed
5. The usual `ReadOk` / scoped-channel flow continues

## Completed work

The libsecret mode is implemented. See the architecture notes above and
`docs/security.md` for the full design and security analysis.

## Planned work

- None currently. See git log for recent libsecret mode implementation.
