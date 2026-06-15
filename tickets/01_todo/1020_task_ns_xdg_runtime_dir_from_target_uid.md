# Task: Point XDG_RUNTIME_DIR at the executing target uid's provisioned dir

Parent: `1000_story_runtime_dir_for_executing_uid`
Depends on: `1010_task_nix_provision_target_runtime_dir`

## Problem

`crates/underwrap-ns/src/env.rs::build_target_env` currently sets
`XDG_RUNTIME_DIR = <xdg_runtime_dir_base>/<caller-uid>` using the **caller's**
uid (`target_identity.uid`, captured pre-switch). The default must instead be
the **executing** uid X's own provisioned runtime dir (1010).

## Design

- In `build_target_env` (default path, i.e. `access_runtime_dir == false`), set
  `XDG_RUNTIME_DIR = <runtime_base>/<X-component>` where `X-component` matches
  the path 1010 provisions. Resolve X from the isolation identity ns already
  has (`target_uid::resolve_isolation` → `Isolation { uid, gid }`), and derive
  the path component the same way the module does (proposed: `getpwuid(X).pw_name`
  → target user name; finalize the scheme jointly with 1010).
- ns already does a `getpwuid` for the caller (`resolve_target_identity`); add
  the analogous lookup for X (name), or pass the resolved `Isolation` + a name
  lookup into `build_target_env`. `build_target_env` currently takes the caller
  `uid`; rework its signature to take what it needs for the X-based path (keep
  it unit-testable — see 1040).
- Keep `XDG_RUNTIME_DIR` in the forced/immutable set (still rejected from
  `envPassthrough`/`envSet`).
- The opt-in caller-session path (`access_runtime_dir == true`) overrides this
  to point at the caller's `/run/user/<caller-uid>` instead (handled in 1030);
  ensure the two paths are mutually exclusive and clearly branched.

## Protocol / config touch-points

- `xdg_runtime_dir_base` (DaemonConfig + ExecBundle + ReadOk wire) stays as the
  plumbing for the base, but its **meaning** changes: it is now the base for the
  *target uid's* provisioned dir (default `/run/underwrap-runtime`), not the
  caller's. Update the doc comments. Consider whether a clearer name is
  warranted (greenfield — renaming is allowed if it improves clarity; not
  required).
- The caller-session opt-in (1030) needs the caller's runtime base too
  (`/run/user`); decide whether that is a second knob or a fixed constant. A
  fixed `/run/user` for the caller side is acceptable.

## Acceptance

- Default-path `XDG_RUNTIME_DIR` resolves to `<base>/<X>` and the uid-X target
  can read/write it (the dir is X-owned via 1010).
- The value is byte-stable and matches the provisioned path exactly.
- Unit tests updated (1040).
