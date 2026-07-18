# underwrap

underwrap brokers secrets to local CLI targets at invocation time, without
handing the reusable secret directly to the caller.

> [!WARNING]
> underwrap is pre-release security software. Its design is intentionally
> defensive, but it has not received an independent end-to-end security audit.
> Read the [security model](docs/security.md) before relying on its guarantees.

**Frontend platform:** NixOS. The privileged wrapper chain depends on the NixOS
module for setgid wrappers, file capabilities, dedicated users, systemd units,
and host hardening. `underwrap-backend` can run on any Linux host.

## The problem

Many CLIs expect a reusable token in an environment variable, config file, or
user keyring. That makes the credential easy to spread through shell sessions,
dotfiles, process environments, diagnostics, and automation frameworks.

underwrap replaces “give the caller the token” with “let the caller invoke a
configured capability.” Its most opinionated use case is an agent-facing narrow
command:

```text
printf '%s\n' 'reviewed' | agent-gh-comment <pr-number>
```

The bounded comment body arrives on stdin; the helper does not accept a path it
could be tricked into reading from its own secret-bearing process. This is a
narrower surface than an unrestricted CLI with `GH_TOKEN` in the agent's
environment.

## How it works

A supported deployment has a central backend and one or more NixOS frontend
instances:

```text
caller
  │
  ▼
underwrap-<instance>             setgid entry stage
  │ Unix socket, effective-GID gate
  ▼
underwrapd                       local broker
  │ HTTPS + mTLS
  ▼
underwrap-backend                resources, policy, admin SPA
```

For each invocation:

1. `underwrap-exec` enters through the instance's setgid binary and connects to
   its `0660` Unix socket.
2. `underwrapd` verifies the peer's effective GID, validates the requested
   wrapper name, and asks the backend to authorize the
   `(runtime client instance, wrapper, resource, variant)` tuple.
3. The backend returns one canonical resource blob when the permission allows
   the invocation.
4. The daemon chooses the configured target. The caller never supplies a target
   path.
5. `underwrap-ns` verifies that fixed fd 20 is an `AF_UNIX` stream socket
   created by the selected instance daemon, then reads the fd 3 bundle. It
   creates PID, mount, IPC, and UTS namespaces; switches to the dedicated UID
   for the `(instance, wrapper)` pair; projects the requested resource slots;
   drops capabilities; and executes the target with a cleared environment.
6. Files or libsecret changes are canonicalized and submitted for backend
   writeback. The backend re-checks policy and writability before persisting
   them.

A resource can expose any combination of:

- **env** — environment variables;
- **files** — credential files in namespace-private tmpfs-backed mounts;
- **libsecret** — a private Secret Service session for tools such as `gh`.

## Security boundaries

underwrap is a secret-delivery system, not a general sandbox.

- The caller is untrusted with respect to the secret.
- The configured target is trusted with the projection it receives. A target
  that prints, logs, stores, or transmits its secret defeats confidentiality.
- Every wrapper runs under a dedicated target UID. This is the primary boundary
  against caller-owned processes reading the target's `/proc` state.
- The default hardened posture requires Linux 6.5+, defaults Yama
  `ptrace_scope=2` with `mkDefault`, and requires encrypted swap or no swap.
  Overriding the Yama value downward weakens the ATTACH-mode claim.
- Root, kernel, backend-host, or target compromise is out of scope.
- `allowedUsers` controls which users receive convenient PATH shims; it is not a
  local access-control list. Backend policy identifies the mTLS runtime client
  instance, not the local Unix user. Add sudo or polkit when local-user
  authorization or a prompt is required.

For agent-facing use, prefer:

```text
underwrap wrapper → narrow helper → real CLI
```

over:

```text
underwrap wrapper → unrestricted CLI
```

See [Agent-facing wrappers](docs/agent-facing-wrappers.md) and the
[wrapper author checklist](docs/wrapper-author-checklist.md).

## Example user experience

A configured wrapper appears as a normal command through a per-user shim:

```bash
jira issue list
```

The shim invokes the instance entry point:

```bash
underwrap-work jira issue list
```

The target receives its configured projection for that invocation; the caller's
shell never receives the token.

## Permission model

The backend stores one permission row per
`(runtime client instance, wrapper, resource)`. Each row has:

- a permission mode: `Locked`, `Unlocked`, `UnlockedUntil`, or
  `UnsafeDebugUntil`;
- a `writable` flag for files/libsecret writeback.

`UnsafeDebugUntil` is the only mode that admits the debug variant. It launches
an intentionally unsafe incident-debug tool with access to the projection, so
it is time-bounded and requires explicit confirmation in the admin UI.

Per-invocation approval is not part of underwrap. Use an OS-level mechanism
when a human prompt is required.

## Development

Enter the development shell and run the focused checks you need:

```bash
nix develop
cargo test --workspace
cargo fmt -- --check
cargo clippy --all-targets --workspace -- -D warnings
cd web && npm test && npm run build
```

The complete flake gate is:

```bash
nix flake check
```

NixOS module and wrapper-boundary changes also require:

```bash
nix build .#checks.x86_64-linux.vm-test
```

## Documentation

Start with the [documentation index](docs/index.md).

- [Getting started](docs/getting-started.md)
- [Configuration](docs/configuration.md)
- [Architecture](docs/architecture.md)
- [Security](docs/security.md)
- [Backend reference](docs/backend.md)
- [Admin UI](docs/admin-ui.md)
- [Testing](docs/testing.md)

## Project status

The wrapper chain, backend, admin SPA, environment projection, file projection,
private Secret Service mode, multi-instance NixOS deployment, and end-to-end VM
tests are implemented. The project remains pre-release while its interfaces,
documentation, operational ergonomics, and security analysis are refined.

## License

Licensed under either:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)); or
- MIT ([LICENSE-MIT](LICENSE-MIT)).

at your option.

Unless explicitly stated otherwise, contributions intentionally submitted for
inclusion are dual-licensed on the same terms.
