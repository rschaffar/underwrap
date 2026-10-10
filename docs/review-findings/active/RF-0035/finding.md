# RF-0035 — PATH-dependent keygen instructions can select an entry wrapper

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

`docs/operations.md:129-131` uses PATH-dependent `sudo underwrap-keygen --instance-config ...`; replacement instructions reuse it at `docs/operations.md:178-186`, and onboarding repeats it at `docs/getting-started.md:183-185`.

The valid instance selector `keygen` passes `nix/modules/underwrap.nix:84-87`.
It creates `/run/wrappers/bin/underwrap-keygen` from underwrap-exec (`nix/modules/underwrap.nix:113-132,196-205`), while the actual keygen helper is separately installed in the system profile (`nix/modules/underwrap.nix:906-916`).
If sudo's executable search, or an equivalent root environment, puts `/run/wrappers/bin` before `/run/current-system/sw/bin`, the documented basename resolves to the wrong program.

The entry parser does not implement the keygen CLI: it interprets `--instance-config` as a wrapper selector rather than a keygen option (`crates/underwrap-exec/src/main.rs:94-105,134-157`).
Depending on entry identity checks, the operator gets wrapper/authentication failure instead of identity installation.

## Consequence and boundary

Enrollment/replacement becomes unreliable for a supported selector under the stated command-resolution environment.
This is trusted generated naming to operator command selection, requiring no malicious actor or PATH injection.
The fixed `demo` tutorial does not itself create `keygen` on a clean host, but its ambiguous command can encounter an already-installed colliding instance.
Not every sudo configuration has this search order.
No private identity was exposed or authorization bypass established.

## Correction and alternatives

Use the module-installed absolute `/run/current-system/sw/bin/underwrap-keygen` consistently in frontend installation/replacement instructions, mirroring the explicit discovery-helper practice in `docs/agent-usage.md:27-33`.
Retain `keygen` as a valid selector and the current entry/helper separation.
Reserving helper names is a broader optional design change, not required here.
This differs from internal generated-object collisions in [RF-0013](../RF-0013/README.md).

## Acceptance criteria

- All relevant frontend operator instructions unambiguously select the helper while preserving its arguments.
- On an authorized disposable NixOS host with instance `keygen`, verify selection under both wrapper-first and system-profile-first lookup orders, including sudo's own search behavior.
- Verify a noncolliding instance control without opening, printing or replacing a private identity.
  Do not infer helper selection merely from a source-shell PATH or perform real credential generation for this wording check.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
