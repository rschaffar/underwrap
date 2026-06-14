# Testing

underwrap has two main layers of automated validation:

- regular Rust tests across the workspace
- a NixOS VM test that exercises the real deployment model

For a project like this, both matter. Unit and integration tests catch protocol and logic regressions; the VM test gives confidence that the NixOS wrapper chain actually behaves the way the design expects.

## Recommended commands

The easiest "run everything that matters" command is now:

```bash
nix flake check
```

That runs the workspace checks defined in `flake.nix`, including:

- Rust tests
- `cargo nextest`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cargo-audit`
- `cargo-deny`
- `cargo-llvm-cov`
- the NixOS VM test

If you prefer running the layers manually before a larger change, use:

```bash
cargo test --workspace
cargo fmt -- --check
cargo clippy --all-targets --workspace -- -D warnings
nix build .#checks.x86_64-linux.vm-test
```

## What the Rust tests cover

The workspace tests cover things such as:

- config parsing and protocol serialization
- IPC framing
- storage backends
- daemon control and wrapper request behavior
- unlock and lock state handling
- fd passing and scoped libsecret channel behavior
- scoped directory channel behavior
- Secret Service data model behavior
- CLI request construction and integration flows
- wrapper and namespace helper logic

These tests are fast and good at catching mistakes in individual components.

## What the VM test covers

The VM test is where the deployment story gets exercised end to end.

It boots a NixOS VM with real underwrap services and then checks properties such as:

- wrapper installation and permissions
- daemon/socket layout
- setgid-based wrapper identity
- successful secret delivery through the wrapper chain
- spoofing resistance
- lock and unlock behavior
- target process isolation properties
- mandatory per-target UID isolation: a full matrix proving that a caller-owned
  peer **and** ancestor cannot inspect a target or its descendants —
  `environ`/`maps`/`smaps`/`auxv`/`mem`, `ptrace(PTRACE_ATTACH)`,
  `process_vm_readv`, and `/proc/<pid>/{fd,cwd,root,exe}` re-open are all denied;
  the world-readable `/proc` residue is secret-free; **root** can still read
  (documented limitation); the target runs as a dedicated `underwrap-target-*`
  uid (never `0`/caller) with zero capabilities; and the dedicated uid comes
  only from the root-owned `/etc/underwrap/target-uids` table (`underwrap-ns`
  holds `cap_setuid`/`cap_setgid`; the table is `root:root 0644`).
  Files/libsecret projection and read-only-grant writeback are also exercised
  under the same mandatory target UID model.
- core dump prevention
- libsecret mode end to end
- directory credential staging and sync
- caller-owned host-namespace inability to list mounted directory credential paths or read files from them
- caller-owned host-namespace inability to list private libsecret runtime mountpoints or reach private sockets
- remote-style instance lifecycle (a second daemon talking to the backend over mTLS)
- absence of secret values in logs

That is much closer to the actual security story underwrap is trying to tell.

## What the tests do **not** prove

Even a strong automated test suite does not equal a security proof.

The tests do not replace:

- careful code review
- threat-model review
- scrutiny of assumptions about Linux kernel behavior
- real-world deployment feedback

This is especially important for a project that makes ambitious claims about local secret handling.

## A useful way to think about confidence

The tests can give you confidence that:

- the intended mechanisms are present
- known regressions are caught
- important deployment details are exercised regularly

They cannot give you absolute confidence that:

- every claim is perfectly worded
- every kernel edge case has been explored
- there are no design mistakes left

That is why the [Security](security.md) document is detailed and why deeper review is still welcome.

## If you change the NixOS module

If your change touches any of these, the VM test should be considered mandatory:

- users / groups / wrapper generation
- socket locations or permissions
- systemd services
- the daemon's outbound mTLS connection layout
- anything related to the wrapper chain

## If you change security-sensitive wrapper behavior

In addition to the VM test, pay close attention to:

- privilege drops
- environment handling
- fd inheritance and closure
- logging
- libsecret channel scoping
- namespace setup

Those are the areas where small mistakes can undermine the intended model.

## Running individual flake checks

If you want one specific gate instead of the whole flake, the main checks are:

```bash
nix build .#checks.x86_64-linux.workspace-test
nix build .#checks.x86_64-linux.workspace-nextest
nix build .#checks.x86_64-linux.workspace-clippy
nix build .#checks.x86_64-linux.workspace-fmt
nix build .#checks.x86_64-linux.workspace-audit
nix build .#checks.x86_64-linux.workspace-deny
nix build .#checks.x86_64-linux.workspace-llvm-cov
nix build .#checks.x86_64-linux.vm-test
```

## Related docs

- [Architecture](architecture.md)
- [Security](security.md)
