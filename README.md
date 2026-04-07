# underwrap

Wrap command execution with just-in-time secret injection and privilege drop.

A daemon holds secrets. Wrappers ask for them over Unix sockets, inject them
into the child process environment, drop privilege, and exec the target tool.

## Quick start

```bash
direnv allow         # auto-load the flake dev shell
# or: nix develop

cargo build          # build all binaries
cargo test --workspace   # 80+ tests
cargo clippy --all-targets --workspace -- -D warnings

nix build .#checks.x86_64-linux.vm-test   # NixOS VM integration test
```

## Architecture

```
crates/
  underwrap-protocol/   # IPC framing, config schema, message types
  underwrap-store/      # StorageBackend trait, plain + encrypted backends
  underwrapd/           # Daemon: sockets, policy, unlock, derivation, audit
  underwrapctl/         # CLI: status, store, unlock, lock
  underwrap-exec/       # Wrapper binary: connect, inject, drop priv, exec
  integration-tests/    # End-to-end test suite
```

### How it works

1. User runs `/run/wrappers/bin/gws-castlabs gmail users.labels.list ...`
2. NixOS `security.wrappers` sets effective GID to `underwrap-gws-castlabs`
3. `underwrap-exec` connects to daemon socket
4. Daemon reads `SO_PEERCRED.gid`, looks up which wrapper owns that GID
5. Daemon checks unlock state, loads secret, applies derivation if configured
6. Returns derived material (e.g. access token, not the raw refresh token)
7. Wrapper injects into env, calls `prctl(PR_SET_DUMPABLE, 0)`, drops privilege, execs target

### Security model

- **Wrapper identity**: kernel-verified via `setgid` + `SO_PEERCRED.gid`. Each wrapper gets a dedicated Unix group. The daemon ignores client-supplied wrapper names when GID identity is available.
- **Socket auth**: UID/GID allowlists, explicit chmod (read=0660, control=0600)
- **Privilege drop**: drops euid, egid, supplementary groups; aborts on failure
- **PR_SET_DUMPABLE=0**: child's `/proc/pid/environ` owned by root, blocks same-UID reads and ptrace
- **Immutability**: `mutable: false` secrets reject overwrites
- **Unlock gates**: `require_unlock: true` blocks reads until `underwrapctl unlock`
- **Atomic writes**: `tempfile::NamedTempFile` + `persist()`, 0600 perms
- **Encrypted at rest**: optional ChaCha20-Poly1305 backend with zeroized master key
- **Audit**: structured events for all operations; denials at `warn` level; secret material never logged
- **OAuth hardening**: hardcoded Google token endpoint (ignores stored `token_uri`), no redirects, error bodies truncated

### NixOS module

```nix
{
  imports = [ underwrap.nixosModules.default ];

  services.underwrap = {
    enable = true;
    package = underwrap.packages.x86_64-linux.default;

    wrappers.gws-castlabs = {
      target = "${pkgs.gws}/bin/gws";
      secret = "google/castlabs";
      injection = { mode = "env"; variable = "GWS_TOKEN"; };
      envVariable = "GWS_TOKEN";
      requireUnlock = true;
    };

    secrets."google/castlabs" = { mutable = true; };
  };
}
```

The module creates per-wrapper groups, installs setgid wrappers via
`security.wrappers`, writes sidecar configs to `/etc/underwrap/wrappers/`,
and runs the daemon under a dedicated `underwrap` user.

### Configuration

YAML-based. See [examples/config.yaml](examples/config.yaml).

## See also

- [TODO.md](TODO.md) — open work items and deferred scope

## License

MIT
