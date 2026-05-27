# Task: Emit a structured audit event on peer-auth cross-check rejection

Part of: hardening follow-up (post-2026-05 epics).

## Problem

`peer_auth::get_peer_cred` now rejects a connection (returns `None`) in
several cases, including the new `SO_PEERCRED` uid/gid **cross-check
failure** — the signature of PID reuse on the fallback path — and a
`SO_PEERPIDFD`-resolved-but-`/proc`-read-failed case. Those rejections
are logged only as a `warn!` inside `peer_auth.rs`.

In `wrapper_flow/intake.rs::authorize_wrapper_peer`, when `get_peer_cred`
returns `None` the daemon writes the wire error
(`"could not determine peer uid"`) and returns **without** calling
`audit::auth_denied`. By contrast, the egid-mismatch path *does* emit a
structured `audit::auth_denied` event (now including the `auth_path`).

So a likely-malicious event (PID-reuse cross-check failure) is *less*
forensically visible than an ordinary egid mismatch — the opposite of
what we want.

## What to do

- Give `peer_auth` a way to distinguish "couldn't read creds at all"
  from "creds read but cross-check failed" (e.g. return a small enum or
  `Result` instead of `Option`, or have `get_peer_cred` emit the audit
  event itself with the `auth_path`).
- Ensure the cross-check-failure rejection produces a structured
  `audit::auth_denied` event (kind `"wrapper"`, a reason like
  `peer-cred-mismatch`, the peer uid/gid if known, and the `auth_path`),
  consistent with the egid-mismatch path.
- Keep the wire error message generic (don't leak which check failed to
  the caller).

## Acceptance

- A cross-check rejection emits a structured audit event, not just a
  `warn!`.
- Unit test covering the cross-check rejection asserts the audit path is
  taken (or, if audit is hard to assert directly, that the new
  enum/Result arm is returned).
- `cargo test --workspace`, clippy, fmt clean.
