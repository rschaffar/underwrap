# Story 2670: make target UID isolation mandatory

## Context

underwrap is greenfield and is being shaped for a polished first release; backwards compatibility with the current optional `isolate` flag is not a constraint.

The current mixed model (`isolate = true` for some wrappers, `false` for others) overcomplicates the Nix module, `underwrap-ns`, tests, and the security documentation. It also creates an awkward global-capability reality: once any wrapper enables isolation, the shared `underwrap-ns` binary already carries the transient `CAP_SETUID`/`CAP_SETGID` surface for all invocations.

## Decision

Targets should always run under a dedicated per-target `underwrap-target-*` UID/GID. A wrapped target must never run as the caller.

The NixOS module should stop exposing an `isolate` knob. Selected access to caller-owned config/state should be expressed explicitly through idmapped access paths.

## Child tickets

- `2680_task_nix_always_isolated_targets.md` — remove the optional Nix module model and always generate target UID authority.
- `2690_task_underwrap_ns_require_isolation.md` — collapse `underwrap-ns` to a single isolated execution path.
- `2700_task_access_paths_config_cleanup.md` — reframe `idmapPaths` as the normal access mechanism for isolated targets.
- `2710_task_docs_tests_always_isolated_model.md` — update docs and VM tests to the mandatory-isolation model.

## Desired outcome

The product story becomes simple:

> underwrap targets always execute as dedicated sandbox users; if a target needs caller files/config, the wrapper declares the allowed paths explicitly.
