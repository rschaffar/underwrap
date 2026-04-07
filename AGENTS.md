# AGENTS.md

## Project overview

underwrap is a secret brokerage system for NixOS. A daemon holds secrets and
serves them to setgid wrapper binaries over Unix sockets. Targets run inside
PID namespaces for process isolation. Rust workspace + Nix flake.

**NixOS only.** The security model depends on `security.wrappers`, declarative
user/group management, and file capabilities. The only non-NixOS component
is `underwrap-backendd` (the remote backend daemon).

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
  underwrap-protocol/    # Shared: IPC framing, config schema, messages
  underwrap-store/       # StorageBackend trait: plain, encrypted, remote
  underwrapd/            # Frontend daemon: identity, unlock, audit
  underwrap-backendd/    # Backend daemon: storage, OAuth derivation
  underwrapctl/          # CLI: status, store, unlock, lock
  underwrap-exec/        # Wrapper stage 1 (setgid): get secret, pipe to stage 2
  underwrap-ns/          # Wrapper stage 2 (cap_sys_admin): PID namespace, exec target
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
- `crates/underwrap-ns/src/main.rs` — PID namespace + hardening
- `crates/underwrapd/src/server.rs` — daemon: identity resolution, unlock, dispatch
- `crates/underwrap-store/src/lib.rs` — StorageBackend trait
- `nix/modules/underwrap.nix` — NixOS module (generates everything)
- `docs/security.md` — every security mechanism explained

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
- Never skip return value checks on privilege-drop syscalls

### Always do

- Verify wrapper identity from `SO_PEERCRED` effective GID (read from `/proc/pid/status`)
- Zeroize secret material after use (`zeroize` crate)
- Set `PR_SET_DUMPABLE=0` on wrapper processes that hold secrets
- Set `RLIMIT_CORE=0` before exec of target
- Use `tempfile::NamedTempFile::persist()` for atomic writes
- Run the full test suite before committing: `cargo test --workspace`
- Run the VM test for NixOS module changes: `nix build .#checks.x86_64-linux.vm-test`

### Ask first

- Before adding new dependencies (especially native/C libraries)
- Before changing the IPC protocol (affects all components)
- Before modifying the NixOS module's user/group/socket structure
- Before changing the wrapper binary chain (underwrap-exec → underwrap-ns)

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

## Planned work

See `docs/libsecret-implementation.md` for the next major feature: a freedesktop
Secret Service D-Bus provider that runs inside the PID namespace, enabling tools
like `gh auth login` to store credentials via libsecret → underwrap backend.

Reference implementation to study: `pass-secret-service` (github.com/grimsteel/pass-secret-service)
— pure Rust, zbus v4, ~3100 lines. Cloned at `~/projects/3rdparty/pass-secret-service`.
