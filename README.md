# underwrap

Wrap command execution with just-in-time secret injection and privilege drop.

`underwrap` is a generic wrapper-oriented secret brokerage system for local
command execution. See [CONCEPT.md](CONCEPT.md) for the full design.

## Status

**Phase 1 — minimal static secret broker.** The core flow works end to end:

- Daemon (`underwrapd`) with dual Unix socket (read + control)
- Store secrets via `underwrapctl store`
- Query status via `underwrapctl status`
- Wrapper binary (`underwrap-exec`) resolves identity from `argv[0]`, contacts
  daemon, injects secret as env var, drops privilege, and execs target
- SO_PEERCRED peer credential logging
- Plain directory storage backend
- YAML configuration

## Quick start (development)

```bash
# Enter the Nix dev shell
nix develop

# Build everything
cargo build

# Run all tests (46 tests)
cargo test --workspace

# Check formatting and lints
cargo fmt -- --check
cargo clippy --all-targets --workspace
```

## Nix build

```bash
# Build all binaries
nix build

# Outputs: result/bin/underwrapd, result/bin/underwrapctl, result/bin/underwrap-exec
ls result/bin/
```

## Architecture

```
crates/
  underwrap-protocol/   # Shared: IPC framing, config schema, message types, policy
  underwrapd/           # Daemon: dual socket listener, storage, policy enforcement
  underwrapctl/         # CLI: status, store secrets
  underwrap-exec/       # Wrapper binary: argv[0] identity, inject, exec
  integration-tests/    # End-to-end test suite
```

### IPC Protocol

Length-prefixed JSON over Unix domain sockets:

- **Read socket** (`/run/underwrap/read.sock`): wrappers request secret material
- **Control socket** (`/run/underwrap/control.sock`): admin operations (store, status)

### Configuration

YAML-based. See [examples/config.yaml](examples/config.yaml).

```yaml
wrappers:
  gh-authorized:
    target: /usr/bin/gh
    secret: github/token
    injection:
      mode: env
      variable: GH_TOKEN

secrets:
  github/token:
    mutable: false
```

### Wrapper Flow

1. User runs `gh-authorized pr list`
2. `underwrap-exec` identifies itself as `gh-authorized` from `argv[0]`
3. Connects to daemon, requests secret for this wrapper
4. Daemon checks policy, returns secret material
5. Wrapper sets `GH_TOKEN` in environment
6. Drops setgid privilege
7. Execs `/usr/bin/gh pr list`

## Test Coverage

46 tests covering:

- **Protocol**: config parsing (8), message serialization (6), IPC framing (2)
- **Storage**: read/write/overwrite, hierarchy, path traversal rejection (7)
- **Integration**: store→read flow, error cases, secret isolation, concurrent
  access, binary-level exec/ctl/daemon tests, large/special payloads (23)

## License

MIT
