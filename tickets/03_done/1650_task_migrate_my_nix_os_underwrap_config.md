# Task: Migrate `~/nix/my_nix_os` to the single-instance underwrap module

Part of: rollout follow-up for the 2026-05 single-instance redesign.

> This ticket targets the operator's **personal** NixOS config repo at
> `~/nix/my_nix_os`, not the underwrap repo. It is filed here so the
> redesign rollout is tracked alongside the module change that forces it.

## Problem

The redesign removed the per-wrapper `addUnsafeDebugTool` NixOS option
(the debug tool is now always present, gated at runtime by the
`UnsafeDebugUntil` permission mode). The personal config still sets
`addUnsafeDebugTool = true` on **every** wrapper in
`~/nix/my_nix_os/nixos/presets/underwrap-robert-work.nix`
(instance `robert`, 11 wrappers: `agent-slack`, `bx`, `jira-castlabs`,
`jira-units-master`, `aws-sso`, `gh-authorized`, `confluence`,
`gws-auth-castlabs`, `gws-castlabs`, `gws-auth-projektbuero`,
`gws-projektbuero`).

With the new module, `addUnsafeDebugTool` is no longer a declared
option, so `nixos-rebuild` will fail to **evaluate** on every host that
imports this preset (`deepthought`, `shame01`, `smallthought`) once the
`underwrap` flake input is bumped to the redesigned revision.

The flake input is a local path
(`underwrap.url = "git+file:///home/robert/projects/projektbuero/underwrap"`),
so the breaking change lands as soon as `flake.lock` is updated.

## What to do

In `~/nix/my_nix_os`:

1. Remove every `addUnsafeDebugTool = true;` line from
   `nixos/presets/underwrap-robert-work.nix`. The rest of each wrapper
   entry (`target`, `resource`, `env`, `files.paths`, `libsecret`,
   `envSet`) is unchanged in the new schema — no other edits needed.
2. `nix flake update underwrap` (or `nix flake lock --update-input
   underwrap`) to pick up the redesigned module + package.
3. `nixos-rebuild build` (or `--flake .#<host>`) for each host that
   imports the preset and confirm it evaluates and builds.
4. Sanity-check the resulting runtime shape on a test host:
   - one setgid binary `/run/wrappers/bin/uw-robert`, one group
     `underwrap-robert`, one socket
     `/run/underwrap-robert/wrapper.sock`;
   - per-user shims on `robert`'s PATH: `<wrapper>` and
     `debug-<wrapper>` for each of the 11 wrappers (no `uw` bare alias —
     that only exists for an instance literally named `default`);
   - `uw-robert jira-castlabs ...` and the bare `jira-castlabs` shim both
     work.
5. Update any operator docs/notes that referenced `addUnsafeDebugTool`.

## Notes for the operator

- **Debug is now a runtime decision.** To inspect a wrapper's projected
  secret, set that `(client_instance, wrapper, resource)` permission to
  `UnsafeDebugUntil` in the admin UI (shortest TTL that fits), then run
  `uw-robert debug <wrapper>` or the `debug-<wrapper>` shim. No rebuild
  needed — this was the whole point of dropping the build-time option.
- The backend (`shame01.boisivpn:18443`) policy is unchanged by this
  migration; only the frontend module shape changed.

## Acceptance

- `nixos-rebuild build` succeeds for every host importing the preset.
- No `addUnsafeDebugTool` remains in `~/nix/my_nix_os`.
- A wrapper runs end-to-end via both `uw-robert <wrapper>` and the bare
  shim; `uw-robert debug <wrapper>` is denied until an `UnsafeDebugUntil`
  permission is set, then prints the projected env.
