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
  underwrap-protocol/    # Shared: IPC framing, config schema, messages, SCM_RIGHTS
  underwrap-store/       # StorageBackend trait: plain, encrypted, remote
  underwrapd/            # Frontend daemon: identity, unlock, audit, secretsd channels
  underwrap-backendd/    # Backend daemon: storage, OAuth derivation
  underwrapctl/          # CLI: status, store, unlock, lock
  underwrap-exec/        # Wrapper stage 1 (setgid): get secret, pipe to stage 2
  underwrap-ns/          # Wrapper stage 2 (cap_sys_admin): PID namespace, exec target
  underwrap-secretsd/    # D-Bus Secret Service provider for libsecret mode
  underwrap-secretsd-store/ # In-memory Secret Service data model + JSON serialization
  integration-tests/     # Cargo integration tests
nix/
  modules/underwrap.nix  # Multi-instance NixOS module
  tests/node.nix         # VM test node configuration
  tests/test.py          # VM test script (Python)
  tests/spoof.py         # Socket spoofing test helper
docs/                    # Documentation (architecture, security, config, etc.)
examples/config.yaml     # Annotated example daemon config
```

## Key files to understand first

- `crates/underwrap-exec/src/main.rs` — two-stage wrapper entry point
- `crates/underwrap-ns/src/main.rs` — PID namespace + hardening + libsecret mode
- `crates/underwrapd/src/server.rs` — daemon: identity resolution, unlock, dispatch
- `crates/underwrapd/src/secretsd_channel.rs` — scoped channel handler for secretsd
- `crates/underwrap-store/src/lib.rs` — StorageBackend trait (read/write/list/delete)
- `crates/underwrap-secretsd/src/main.rs` — D-Bus Secret Service provider
- `crates/underwrap-secretsd/src/dbus_service.rs` — D-Bus interface implementations
- `nix/modules/underwrap.nix` — NixOS module (generates everything)
- `docs/security.md` — every security mechanism explained (including libsecret mode)

## Code conventions

- Rust 2024 edition, `anyhow` for errors, `tracing` for logging
- Zero clippy warnings enforced (`-D warnings`)
- `cargo fmt` enforced
- Secret material is NEVER logged — audit events log secret IDs, not values
- All `unsafe` is for well-understood libc FFI only (setgid, setuid, prctl, etc.)
- Return values from privilege-drop syscalls are always checked
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

- Verify wrapper identity from `SO_PEERCRED` effective GID (read from `/proc/pid/status`)
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
- VM test: `nix build .#checks.x86_64-linux.vm-test` — boots a real NixOS VM, tests
  both local and remote backends, unprivileged user, spoofing resistance, PID namespace
  isolation, memory protection, core dump prevention, SSH tunnel lifecycle

## Architecture notes

**Two-stage wrapper chain:**
`underwrap-exec` (setgid) → pipe fd 3 → `underwrap-ns` (cap_sys_admin) → target in PID namespace

**Identity model:** setgid group → `SO_PEERCRED` effective GID → daemon GID→wrapper map

**Remote backend:** frontend daemon connects to `underwrap-backendd` via SSH-tunneled Unix socket.
Tunnel connect = unlock. Tunnel disconnect = lock.

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
7. All libsecret read/write: secretsd → daemon channel → StorageBackend
8. fd 4 (channel) is CLOSED before exec'ing the target
9. On namespace exit: kernel SIGKILL kills dbus-daemon + secretsd

See `docs/security.md` "Design invariants" and "Secretsd channel" for security analysis.

**Bootstrap flow** (e.g., `gh auth login` via libsecret wrapper):
```
1. User runs:  work-gh auth login
2. underwrap-exec gets secret + secretsd channel fd from daemon
3. underwrap-ns creates namespace, launches dbus-daemon + secretsd
4. secretsd loads metadata from backend, signals readiness
5. gh does OAuth flow, calls libsecret CreateItem to store token
6. secretsd writes token to backend via channel (encrypted/remote)
7. Next run: gh calls SearchItems + GetSecret, gets token from backend
```

## Completed work

The libsecret mode is implemented. See the architecture notes above and
`docs/security.md` for the full design and security analysis.

## Planned work

- None currently. See git log for recent libsecret mode implementation.
