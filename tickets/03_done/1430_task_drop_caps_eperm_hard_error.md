# Task: Make `drop_caps` bounding-set EPERM a hard error

Part of: `epic_frontend_security_hardening_2026-05`.

## Problem

`crates/frontend-ns/src/caps.rs:26-36` — `drop_caps` walks
`0..64` calling `PR_CAPBSET_DROP` for each capability. The loop
breaks on `EINVAL` (correct: the kernel uses EINVAL to signal
"no more capabilities exist") and on `EPERM`:

```rust
Some(libc::EPERM) => break,
```

`EPERM` from `PR_CAPBSET_DROP` means the caller does not hold
`CAP_SETPCAP`. Today the NixOS module installs underwrap-ns with
`capabilities = "cap_sys_admin,cap_setpcap+ep"`
(`nix/modules/underwrap.nix`), so this never fires. But the
project's "Never silent fallback to a weaker mode" rule
(`docs/security.md` §6) is uncompromising for a reason: if the
file caps ever shrink — by config edit, packaging mistake, or a
future hardening attempt that removes `cap_setpcap` thinking it
isn't needed — the current code silently leaves the bounding set
populated. That is exactly the failure mode the rule is meant
to prevent.

The `EINVAL` branch is the correct loop-terminator. `EPERM` should
be a hard error.

## What to do

1. In `crates/frontend-ns/src/caps.rs`, change the `EPERM` arm of
   the `PR_CAPBSET_DROP` match to return `Err`:

   ```rust
   Some(libc::EPERM) => return Err(format!(
       "prctl(PR_CAPBSET_DROP, {cap}) returned EPERM — underwrap-ns \
        requires cap_setpcap; check the NixOS module's file capabilities",
   )),
   ```

2. Keep the `EINVAL` break as the loop terminator.

3. Add a unit test (or wire one through `tests.rs` next to
   `caps.rs`) that asserts the function returns `Err` when invoked
   from a context without `CAP_SETPCAP`. The cleanest way is to run
   the test from a child process started with explicit
   `prctl(PR_CAPBSET_DROP, CAP_SETPCAP)` already done — that is, drop
   `CAP_SETPCAP` first, then call `drop_caps`, and assert `Err`. Skip
   the test if the test binary can't establish those preconditions
   (no `cap_setpcap` to begin with).

4. While here: the ambient-clear `EINVAL` tolerance
   (`caps.rs:21-24`) is fine — older kernels lack `PR_CAP_AMBIENT`
   and treating that as success is correct. Leave it alone but add
   a comment noting why `EINVAL` is the only acceptable failure
   here.

## Acceptance

- `cargo test --workspace`
- `cargo clippy --all-targets --workspace -- -D warnings`
- New test demonstrating `drop_caps` errors when `cap_setpcap` is
  absent (skipped when the precondition can't be set up).
- VM test green: underwrap-ns continues to drop caps successfully
  under the supported configuration.

## Resolution (2026-05-27) — premise was wrong, resolved differently

The VM test disproved this ticket's central assumption. NixOS
`security.wrappers` (`setcap-wrapper.c`) **deliberately strips
`CAP_SETPCAP`** from the ambient set it grants the wrapped binary
("cap_setpcap in set, skipping it"). So `underwrap-ns` never receives
`CAP_SETPCAP` even though the module lists `cap_setpcap+ep`;
`PR_CAPBSET_DROP` always returns `EPERM`, and the original silent
`break` was masking that the bounding-set drop has never run in this
deployment. Making `EPERM` fatal breaks every wrapper invocation.

Decision (operator): keep `EPERM` non-fatal, but document **why** it is
expected in `caps.rs`, and make explicit that the security boundary is
the final `capset` (clears permitted/effective/inheritable to empty)
plus `PR_SET_NO_NEW_PRIVS` — not `PR_CAPBSET_DROP`. The bounding set
being non-empty grants nothing once permitted is empty and no-new-privs
is set.

Delivered instead of the hard error:
- An accurate `EPERM => break` comment in `caps.rs` explaining the
  NixOS-wrapper interaction.
- `caps/tests.rs::drop_caps_clears_effective_without_cap_setpcap`,
  which verifies the real invariant: `drop_caps` succeeds and leaves an
  empty effective capability set even without `CAP_SETPCAP`.
