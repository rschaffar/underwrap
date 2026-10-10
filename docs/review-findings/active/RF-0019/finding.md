# RF-0019 — A held peer pidfd does not pin numeric procfs lookup

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

`crates/underwrapd/src/peer_auth.rs:152-188` acquires SO_PEERPIDFD, reads its numeric PID from fdinfo, opens `/proc/<number>/status`, then drops the pidfd before parsing.
If the peer exits/is reaped after the first read and the number is reused before the numeric open, procfs can identify a replacement.
Holding the original PID object does not reserve its number.

`finish_peer_cred`/`creds_consistent` compare UID/GID values rather than process-object identity (`crates/underwrapd/src/peer_auth.rs:210-243,350-384`).
A same-credential replacement with the required effective GID can satisfy these comparisons and `crates/underwrapd/src/wrapper_flow/intake.rs:63-84`.
There is no post-status validation of the original peer.
Comments/logging at `crates/underwrapd/src/peer_auth.rs:14-16,157-170,320-324,400-404`, `docs/local-protocol.md:15` and `nix/modules/underwrap.nix:858-864` overstate numeric pinning.

## Consequence and boundary

The accepted defect is wrong-process credential attribution by the primitive and a false race-free claim.
Already-dead-at-first-read, acquisition and ordinary read failures still reject; only ENOPROTOOPT permits fallback (`crates/underwrapd/src/peer_auth.rs:177-204,290-297`).
Those fail-closed properties remain real.

The supported chain imposes separate prerequisites absent from the forced-reuse witness:

- The socket is instance-group 0660, and ordinary caller convenience shims do not grant group membership (`crates/underwrapd/src/server.rs:201-207`; `crates/underwrapd/src/startup.rs:340-383`; `nix/modules/underwrap.nix:182-217,518-533,869-879`).
  U cannot simply attach an arbitrary client.
- The trusted setgid entry internally creates the UnixStream and completes Read/ReadOk and resource-channel receipt before returning (`crates/underwrap-exec/src/main.rs:101-108,247-333`).
  It accepts no caller-supplied client socket, transfers no such socket to U and does not fork during this exchange.
- Its first source-level fork is later, after `request_secret` has returned/dropped the stream and after permanent group restoration (`crates/underwrap-exec/src/main.rs:107-131,417-486,530-568`).
  Death during authentication normally closes the sole client endpoint.
  A buffered request does not give U an endpoint to receive ReadOk/SCM_RIGHTS or issue Checkout.
  A new entry does not inherit the old socket.
- Direct namespace invocation still requires the authenticated daemon-created resource capability (`crates/underwrap-ns/src/main.rs:85-131`; `crates/underwrap-ns/src/handoff.rs:26-35,146-167`).

The prior forced-reuse witness deliberately retains both socketpair endpoints and uses its own group, not the protected instance group.
Yama, setgid protections and non-dumpability were not bypassed.
Giving U the group, ptrace/root authority, a leaked endpoint or a modified entry flow would change the model; none is an established supported prerequisite.

## Correction and alternatives

Require an actually established binding between the status snapshot and original peer throughout lookup, rejecting peer death/reuse without downgrade.
Correct comments/docs/logging independently of the selected hardening mechanism.
Preserve effective-GID checks and socket/descriptor controls.
Engineering priority may exceed the evidenced impact, but this is not a verified confidentiality bypass.

## Acceptance criteria

- A deterministic credential-resolution regression must reject original-peer death/reuse even when replacement UID/GID values match.
  Merely asserting `PeerPidfd` mode is insufficient.
- Retain live-original-peer acceptance, unsupported-option fallback classification and acquired-path-error/no-fallback controls.
- Retain positive retained-endpoint and negative sole-client lifetime controls.
  Any future bypass claim must separately demonstrate socket admission and usable transport through the unchanged supported chain, without an extra leak or out-of-model authority.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
