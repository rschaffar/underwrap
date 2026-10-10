# RF-0013 — Instance selectors collide with internal generated names

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

The instance grammar admits `ns` and `runtime` (`nix/modules/underwrap.nix:84-87`).
Generated global objects do not reserve their internal names.

For `ns`, the instance entry generates `security.wrappers.underwrap-ns` from underwrap-exec with setgid/instance-group settings (`nix/modules/underwrap.nix:125-128,199-205`).
The shared namespace-stage wrapper uses the same key with capability-bearing namespace settings (`nix/modules/underwrap.nix:897-903`).
Forced actual-module toplevel evaluation fails specifically because setuid/setgid and capabilities are mutually exclusive; this is stronger and more precise than the original expectation of a generic option merge failure.

For `runtime`, the default target-runtime base is `/run/underwrap-runtime`.
An instance with that name generates a daemon-owned directory at that path.
With at least one wrapper anywhere, the module also emits the root-owned target-runtime-base rule for exactly that path (`nix/modules/underwrap.nix:199-217,675-681,692-695`).
Both ownership contracts cannot hold simultaneously.

## Consequence and boundary

The affected boundary is trusted configuration selectors to global wrapper/directory identity.
The `ns` case blocks the configuration rather than producing a working privileged wrapper.
The `runtime` case can leave the daemon without its expected writable socket directory or leave the target-runtime base without documented root ownership, depending on effective rule ordering.
Actual deployed ordering/outcome is not established.
Accounts, daemons and configuration are trusted; this evidence does not demonstrate an unprivileged escalation.

This is distinct from documented per-user alias collisions and intentional helper basename shadowing.
The latter has an explicit-path remedy, including [RF-0035](../RF-0035/README.md); it cannot solve two definitions of the same internal security-wrapper key.

## Correction and alternatives

Make internal wrapper/runtime namespaces disjoint, or detect overlaps before generation with explicit documented restrictions.
Consider configured runtime bases as well as the two default-trigger names.
Preserve explicit-instance invocation and intentional `info` helper handling; broad naming redesign is not implicitly selected.

## Acceptance criteria

- Force real-module toplevel evaluation for `ns` and a noncolliding selector; rejection, if chosen, must come from the intended validation rather than the downstream wrapper assertion.
- For `runtime` with a wrapper, ensure each directory has one coherent ownership contract; inspect generated rules and verify exact owner/mode in a disposable activation.
- Keep noncolliding default/custom-base controls and supported helper naming behavior.
  No new activation or fix validation is claimed complete.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
