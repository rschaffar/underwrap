# Task: Force `HOME` and XDG dirs to trusted defaults; add instance-level `XDG_RUNTIME_DIR` base

## Background

`underwrap-ns` launches the final target with `env_clear()`, but the
current default passthrough whitelist still reintroduces these caller-controlled
variables:

- `HOME`
- `XDG_RUNTIME_DIR`
- `XDG_CONFIG_HOME`
- `XDG_DATA_HOME`
- `XDG_CACHE_HOME`
- `XDG_STATE_HOME`

That creates two problems:

1. **Target behavior steering.** A caller can redirect config/state/cache lookups
   to attacker-chosen locations.
2. **Inconsistent home semantics.** Files-slot mounts are already anchored to the
   real passwd home via `getpwuid(getuid())`, while the target environment may
   advertise a different `HOME` / `XDG_*`.

This is a greenfield hardening fix for the first release. Backward
compatibility is not a constraint here.

## Goal

Make target-home semantics deterministic and trusted by default:

- `HOME` and the XDG directory variables are **not** inherited from the
  caller.
- `HOME` plus the base XDG directories are forced to the usual defaults derived
  from a trusted home directory.
- `XDG_RUNTIME_DIR` is forced to a trusted runtime path derived from an
  instance-level base plus the numeric uid.
- A wrapper may still override `HOME` and the base XDG directories
  intentionally via `envSet`.
- `XDG_RUNTIME_DIR` is **not** configurable via `envSet`; it is controlled only
  by the new dedicated instance-level knob.
- Attempting to place any of these names in `envPassthrough` is a **hard
  error**.
- Attempting to place `XDG_RUNTIME_DIR` in `envSet` is also a **hard error**.

## Scope

### 1. Force trusted defaults in `frontend-ns`

Change target-environment construction so the final target always sees trusted
values for:

- `HOME`
- `XDG_RUNTIME_DIR`
- `XDG_CONFIG_HOME`
- `XDG_DATA_HOME`
- `XDG_CACHE_HOME`
- `XDG_STATE_HOME`

Implementation shape:

- Resolve the passwd home exactly once, as `underwrap-ns` already does today.
- Compute an **effective home**:
  - `exec_bundle.env_set["HOME"]` if present
  - otherwise the passwd home
- Compute the trusted runtime dir from a dedicated base plus the numeric uid:
  - `XDG_RUNTIME_DIR = <xdg_runtime_dir_base>/<uid>`
- Populate forced defaults before general passthrough handling:
  - `HOME = <effective home>`
  - `XDG_RUNTIME_DIR = <xdg_runtime_dir_base>/<uid>`
  - `XDG_CONFIG_HOME = <effective home>/.config`
  - `XDG_DATA_HOME = <effective home>/.local/share`
  - `XDG_CACHE_HOME = <effective home>/.cache`
  - `XDG_STATE_HOME = <effective home>/.local/state`
- Then allow explicit `envSet` entries for `HOME`, `XDG_CONFIG_HOME`,
  `XDG_DATA_HOME`, `XDG_CACHE_HOME`, and `XDG_STATE_HOME` to override the
  derived defaults.
- `XDG_RUNTIME_DIR` must not be overridden by `envSet`.
- When applying `exec_bundle.env_passthrough`, ignore these six names even if
  they somehow appear there anyway; `frontend-ns` remains the final authority.
- Caller `HOME` / `XDG_*` values must never influence the final target env.

Recommended refactor:

- Change `crates/frontend-ns/src/env.rs::build_target_env` so it receives the
  resolved passwd home plus the trusted runtime-dir inputs (or a precomputed
  helper struct) instead of deriving everything from the caller environment
  alone.
- Keep the forcing logic centralized in `env.rs`; `main.rs` should only resolve
  the passwd home / uid and pass them in.
- Because `XDG_RUNTIME_DIR` now depends on instance configuration rather than
  only local process state, thread that configuration through the exec→ns
  handoff cleanly. Do not smuggle it via the caller environment.

### 2. Add an instance-level `xdgRuntimeDirBase` knob

Add a dedicated instance-level NixOS option:

- `services.underwrap.instances.<name>.xdgRuntimeDirBase`

Semantics:

- type: string absolute filesystem path
- default: `/run/user`
- used to derive the target runtime dir as `<xdgRuntimeDirBase>/<uid>`

Plumb this value into the generated wrapper sidecar config for every wrapper in
that instance, validate it in `frontend-exec`, and carry it through the
exec→ns handoff explicitly.

Validation / cleanliness requirements:

- reject non-absolute values in the loader as defense in depth
- the knob is instance-level, not per-wrapper
- `XDG_RUNTIME_DIR` is computed from this knob plus the runtime uid, never from
  `envSet` or caller env

### 3. Remove the forced names from the default passthrough whitelist

Update `crates/frontend-ns/src/env.rs` so `DEFAULT_PASSTHROUGH` no longer
contains:

- `HOME`
- `XDG_RUNTIME_DIR`
- `XDG_CONFIG_HOME`
- `XDG_DATA_HOME`
- `XDG_CACHE_HOME`
- `XDG_STATE_HOME`

### 4. Reject `envPassthrough` / `envSet` use of forced names

Make these names invalid in wrapper config `envPassthrough`:

- `HOME`
- `XDG_RUNTIME_DIR`
- `XDG_CONFIG_HOME`
- `XDG_DATA_HOME`
- `XDG_CACHE_HOME`
- `XDG_STATE_HOME`

Additionally, make `XDG_RUNTIME_DIR` invalid in wrapper config `envSet`.

Enforce that in both places:

#### NixOS module

In `nix/modules/underwrap.nix`, add assertions that reject any wrapper whose
`envPassthrough` contains one of the forced names. The error message should tell
operators to use `envSet` for `HOME` / base `XDG_*` overrides and to use the
instance-level `xdgRuntimeDirBase` knob for `XDG_RUNTIME_DIR`.

Also reject any wrapper whose `envSet` contains `XDG_RUNTIME_DIR`; that value
must come only from `xdgRuntimeDirBase`.

#### `frontend-exec` sidecar loader

Add matching validation in `crates/frontend-exec/src/main.rs` after decoding the
wrapper sidecar config. This is defense in depth for non-module-produced or
hand-edited configs; the binary should fail loudly rather than silently accept a
caller-controlled home/config path or runtime-dir override.

Do **not** silently ignore the bad passthrough entry in the loader. Reject it
with a clear error. Separately, keep the `frontend-ns` runtime filtering above
as defense in depth.

### 5. Keep files-slot mount anchoring unchanged

Do **not** change directory mount target resolution in this ticket.

Files-slot mount paths remain relative to the real passwd home as resolved by
`underwrap-ns`, not relative to `envSet.HOME`.

This preserves the existing security property that mount placement is anchored to
kernel-resolved user identity rather than to environment variables.

Document this clearly: if an operator explicitly sets `envSet.HOME` away from
the passwd home for a wrapper that also uses `files.paths`, they are taking
responsibility for that divergence.

### 6. Update tests

#### Rust unit tests

Update `crates/frontend-ns/src/env/tests.rs` to cover at least:

1. **Caller spoof ignored**
   - host env supplies attacker-chosen `HOME` and `XDG_*`
   - target env gets trusted forced defaults instead
   - `XDG_RUNTIME_DIR` resolves from the configured base plus uid, not the
     caller value
2. **`envSet.HOME` override**
   - explicit `envSet.HOME` wins over passwd home
   - unset `XDG_*` derive from that overridden `HOME`
3. **Explicit `envSet.XDG_*` overrides**
   - each explicit base-dir `envSet.XDG_*` wins over the derived default
4. **`XDG_RUNTIME_DIR` is not overrideable via `envSet`**
   - bad config is rejected before launch
5. **General passthrough still works**
   - unrelated allowlisted vars like `LANG` still pass through
6. **Secret env injection still wins last**
   - no regression in existing env-slot behavior

Add `frontend-exec` tests for the new sidecar-config validation:

- `env_passthrough: ["HOME"]` is rejected
- same for each forced `XDG_*` name including `XDG_RUNTIME_DIR`
- `env_set: { XDG_RUNTIME_DIR: ... }` is rejected
- non-absolute `xdg_runtime_dir_base` is rejected
- normal passthrough entries and a normal absolute runtime-dir base still load
  successfully

Follow the sibling-file `tests.rs` layout from `AGENTS.md`.

#### VM test

Extend the NixOS VM test to prove the end-to-end behavior from a real wrapper:

- invoke a probe wrapper with caller-supplied
  `HOME=/tmp/evil-home`, `XDG_RUNTIME_DIR=/tmp/evil-run`,
  `XDG_CONFIG_HOME=/tmp/evil-config`, etc.
- assert the target sees the forced trusted values instead
- assert `XDG_RUNTIME_DIR` is `<configured-base>/<uid>`
- add one wrapper fixture with explicit `envSet.HOME` and/or explicit
  `envSet.XDG_*` and assert those configured values are what the target sees
- add one instance (or wrapper under a dedicated instance) with a non-default
  `xdgRuntimeDirBase` and assert the target sees that base plus uid
- assert the existing `UNDERWRAP_*` non-leak behavior still holds

Prefer reusing the existing env-probe pattern rather than inventing a large new
fixture stack.

### 7. Update documentation

Update the docs anywhere they currently imply that `HOME` is part of the default
safe passthrough set.

At minimum:

- `docs/security.md`
- `docs/configuration.md`
- `docs/wrapper-author-checklist.md`
- `nix/modules/underwrap.nix` option text for `envPassthrough`

The new guidance should be:

- `HOME` and base `XDG_*` are forced to trusted defaults
- `XDG_RUNTIME_DIR` is forced to `<xdgRuntimeDirBase>/<uid>`
- use `envSet` for intentional `HOME` / base `XDG_*` overrides
- use the instance-level `xdgRuntimeDirBase` knob for `XDG_RUNTIME_DIR`
- `envPassthrough` is rejected for those names
- `envSet.XDG_RUNTIME_DIR` is rejected
- `PATH` is still not inherited and still requires explicit trusted config

## Out of scope

- Changing files-slot mount placement semantics
- Adding new path-shape validation rules for `envSet.HOME` / base-dir
  `envSet.XDG_*`
- Changing any backend or daemon↔backend runtime API code
- Reworking broader session-socket behavior beyond forcing `XDG_RUNTIME_DIR`
  itself

## Acceptance criteria

- [ ] An instance-level `xdgRuntimeDirBase` option exists, defaults to
      `/run/user`, and is plumbed cleanly into the wrapper launch path.
- [ ] `crates/frontend-ns/src/env.rs` no longer passes caller `HOME` or caller
      `XDG_RUNTIME_DIR` / `XDG_CONFIG_HOME` / `XDG_DATA_HOME` /
      `XDG_CACHE_HOME` / `XDG_STATE_HOME` through the default whitelist.
- [ ] `underwrap-ns` builds the final target env using trusted defaults for
      those six names, with `envSet` as the only override mechanism for `HOME`
      and the base XDG dirs, and it ignores those names if they somehow appear
      in `env_passthrough` anyway.
- [ ] `XDG_RUNTIME_DIR` is always `<xdgRuntimeDirBase>/<uid>` and cannot be
      overridden through `envSet`.
- [ ] `nix/modules/underwrap.nix` rejects those six names in
      `envPassthrough` with a clear message telling the operator to use
      `envSet` or `xdgRuntimeDirBase` as appropriate.
- [ ] `nix/modules/underwrap.nix` rejects `envSet.XDG_RUNTIME_DIR`.
- [ ] `crates/frontend-exec/src/main.rs` rejects sidecar configs whose
      `env_passthrough` contains any of those six names, rejects
      `env_set.XDG_RUNTIME_DIR`, and rejects a non-absolute
      `xdg_runtime_dir_base`.
- [ ] Unit tests cover caller spoof rejection plus `envSet.HOME` /
      base-dir `envSet.XDG_*` override behavior and `XDG_RUNTIME_DIR` forcing.
- [ ] VM coverage proves end-to-end that spoofed caller `HOME` / `XDG_*`
      values do not reach the target and that `XDG_RUNTIME_DIR` is derived from
      the configured base plus uid.
- [ ] Documentation no longer describes `HOME` or `XDG_RUNTIME_DIR` as part of
      the default safe passthrough set.
- [ ] `cargo test -p frontend-ns` passes.
- [ ] `cargo test -p frontend-exec` passes.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo fmt -- --check` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.

## Notes for the implementer

- Keep this a frontend hardening change. The backend and daemon↔backend API do
  not need to change, but the local exec→ns handoff may grow a field if that is
  the cleanest way to carry `xdg_runtime_dir_base`.
- Be explicit about ordering inside env construction: trusted defaults first,
  explicit `envSet` overrides for `HOME` / base XDG dirs after that, secret env
  bindings last.
- When updating docs, mention the subtle but intentional distinction:
  files-slot mount targets are still anchored to the passwd home even if a
  wrapper intentionally overrides `HOME` via `envSet`.
