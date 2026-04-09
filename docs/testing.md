# Testing

underwrap has two main layers of automated validation:

- regular Rust tests across the workspace
- a NixOS VM test that exercises the real deployment model

For a project like this, both matter. Unit and integration tests catch protocol and logic regressions; the VM test gives confidence that the NixOS wrapper chain actually behaves the way the design expects.

## Recommended commands

Before trusting a meaningful change, run:

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
- daemon read/write behavior
- unlock and lock state handling
- fd passing and scoped libsecret channel behavior
- scoped directory channel behavior
- Secret Service data model behavior

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
- core dump prevention
- libsecret mode end to end
- directory credential staging and sync
- same-UID host-namespace inability to list mounted directory credential paths or read files from them
- same-UID host-namespace inability to list private libsecret runtime mountpoints or reach private sockets
- remote backend tunnel lifecycle
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
- remote tunnel layout
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

## Related docs

- [Architecture](architecture.md)
- [Security](security.md)
