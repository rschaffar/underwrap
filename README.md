# underwrap

Wrap command execution with just-in-time secret injection and privilege drop.

`underwrap` is a generic wrapper-oriented secret brokerage system for local
command execution. See [CONCEPT.md](CONCEPT.md) for the full design.

## Status

All concept phases implemented:

- **Phase 1**: Minimal static secret broker — daemon, storage, wrappers, env injection
- **Phase 2**: Unlock support — time-based, scoped, per-wrapper unlock/lock
- **Phase 3**: Mutable secrets + audit logging — atomic writes, immutability enforcement, structured audit events
- **Phase 4**: Derived credentials — OAuth refresh → access token with caching
- **Phase 5**: Encrypted backend — ChaCha20-Poly1305 with master key file
- **Phase 6**: NixOS module, VM tests, examples

## Quick start (development)

```bash
# Enter the Nix dev shell
nix develop

# Build everything
cargo build

# Run all tests
cargo test --workspace

# Check formatting and lints
cargo fmt -- --check
cargo clippy --all-targets --workspace

# Run NixOS VM integration test
nix build .#checks.x86_64-linux.vm-test
```

## Architecture

```
crates/
  underwrap-protocol/   # Shared: IPC framing, config schema, message types
  underwrapd/           # Daemon: sockets, storage, unlock, derivation, audit
  underwrapctl/         # CLI: status, store, unlock, lock
  underwrap-exec/       # Wrapper binary: argv[0] identity, inject, exec
  integration-tests/    # End-to-end test suite
```

### Wrapper Flow

1. User runs `gws-castlabs gmail users.labels.list ...`
2. `underwrap-exec` identifies itself as `gws-castlabs` from `argv[0]`
3. Connects to daemon, requests secret for this wrapper
4. Daemon checks policy: peer auth, unlock state
5. Daemon loads stored authorized-user JSON
6. Daemon derives: exchanges refresh token → fresh access token (cached)
7. Returns access token only (not the refresh token)
8. Wrapper sets `GOOGLE_WORKSPACE_CLI_TOKEN` in environment
9. Drops setgid privilege
10. Execs `/usr/bin/gws gmail users.labels.list ...`

### Configuration

YAML-based. See [examples/config.yaml](examples/config.yaml).

### Security Model

- **Socket auth**: UID/GID allowlists via `SO_PEERCRED`
- **Socket permissions**: explicit chmod (read=0660, control=0600)
- **Path safety**: absolute paths, traversal, null bytes all rejected
- **Immutability**: `mutable: false` enforced — cannot overwrite
- **Unlock gates**: `require_unlock: true` blocks reads until `underwrapctl unlock`
- **Privilege drop**: `setgid`/`setgroups` return values checked, abort on failure
- **Atomic writes**: temp file + rename, 0600 perms
- **Encrypted at rest**: optional ChaCha20-Poly1305 backend
- **Audit trail**: structured events for all operations, never includes secret material
- **No log leaks**: secret material never appears in daemon logs

### NixOS Module

```nix
{
  imports = [ underwrap.nixosModules.default ];

  services.underwrap = {
    enable = true;
    package = underwrap.packages.x86_64-linux.default;

    encryption = {
      enable = true;
      keyFile = "/var/lib/underwrap/master.key";
    };

    wrappers.gws-castlabs = {
      target = "${pkgs.gws}/bin/gws";
      secret = "google/castlabs";
      injection = { mode = "env"; variable = "GOOGLE_WORKSPACE_CLI_TOKEN"; };
      envVariable = "GOOGLE_WORKSPACE_CLI_TOKEN";
      requireUnlock = true;
    };

    secrets."google/castlabs" = { mutable = true; };
  };
}
```

## Test Coverage

80+ tests covering:

- **Protocol**: config parsing, message serde, IPC framing, injection modes
- **Storage**: plain + encrypted backends, path traversal, atomicity, wrong-key rejection
- **Unlock**: time-based expiry, scopes, lock/unlock lifecycle
- **Derivation**: OAuth JSON parsing, token cache, passthrough
- **Integration**: full daemon lifecycle, binary exec, concurrent access
- **NixOS VM**: systemd service, socket perms, unlock flow, immutability, audit trail, no-leak check

## License

MIT
