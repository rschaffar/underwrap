# Task: Check `MSG_CTRUNC` in `recv_fd`

Part of: `epic_frontend_security_hardening_2026-05`.

## Problem

`crates/shared-protocol/src/scm_rights.rs:60-99` (`recv_fd`) sizes
its ancillary buffer for exactly one fd:

```rust
let cmsg_space = unsafe { CMSG_SPACE(mem::size_of::<RawFd>() as u32) } as usize;
```

If the sender ever sent more than one fd in a single `sendmsg`, the
kernel would deliver the first fd(s) that fit and set `MSG_CTRUNC`
in `msg.msg_flags`. The current code never inspects `msg_flags`, so
ancillary truncation would go unnoticed.

The protocol only sends one fd per Checkout / open call today, so
this isn't an active bug. But:

1. A future protocol change (e.g. sending the channel fd *and* a
   companion fd in one message) would silently lose the second fd
   if the receive side wasn't updated in lockstep.
2. Even today, a misbehaving sender or a kernel quirk that
   appended extra ancillary data would be silently swallowed
   rather than rejected. "No silent fallback" applies here too.

The fix is one branch.

## What to do

In `crates/shared-protocol/src/scm_rights.rs`:

1. After the `recvmsg` call returns success, inspect
   `msg.msg_flags & libc::MSG_CTRUNC`. If non-zero, return
   `Err(anyhow!("recvmsg ancillary truncated"))`.

2. While here: also reject the case where `cmsg.cmsg_len` is
   smaller than `CMSG_LEN(size_of::<RawFd>() as u32)`. The current
   code reads one `RawFd` out of `CMSG_DATA(cmsg)` without
   confirming the cmsg actually carries that many bytes. Almost
   certainly always true in practice, but a one-line check.

3. Consider adding `MSG_CMSG_CLOEXEC` to the `recvmsg` flags **only**
   on the daemon side — the wrapper-exec receiver explicitly passes
   the fd onward to underwrap-ns via `dup2`, which is incompatible
   with CLOEXEC. The cleanest shape is a `recv_fd_cloexec` variant
   used by daemon code, with the existing `recv_fd` left for the
   wrapper-side inheritance use case. Or pass a `cloexec: bool`
   parameter. Pick whichever reads better.

4. The `send_fd` side has no symmetric issue (the sender chooses
   exactly what to put in the cmsg buffer). Leave it.

## Acceptance

- `cargo test --workspace`
- `cargo clippy --all-targets --workspace -- -D warnings`
- New unit test in `scm_rights/tests.rs`: craft a sender that sends
  two fds with a receiver buffer sized for one, assert `recv_fd`
  returns `Err`.
- Daemon-side `recv_fd` callsites switched to the CLOEXEC variant
  (if implemented).
