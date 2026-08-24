# Make the aggregate flake gate match its stated coverage

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-026
Source themes: C-006
Approved prerequisite: D-182 (bounded O-019 system-matrix choice; system half of U-003)

## Outcome

The flake advertises only the intentionally supported Linux systems and its
current-system aggregate gate deterministically runs the SPA unit/build checks
and evaluates both shipped NixOS examples through the real frontend module.
Published test instructions state exactly what the aggregate covers and which
browser and live-advisory checks remain separate.

## Scope

- Replace the flake-utils default system set with exactly `x86_64-linux` and
  `aarch64-linux`, as approved by D-182.
- Reuse one pinned npm dependency closure for the existing SPA package build and
  a deterministic Vitest check.
- Expose explicit `web-build` and `web-test` flake checks without changing SPA
  test or production-build semantics.
- Replace the shallow `examples-nix-parse` check with
  `examples-nixos-eval`, evaluating both complete examples through
  `self.nixosModules.default`, the selected system package, and forced
  `system.build.toplevel` assertions.
- Update `README.md` and `docs/testing.md` so the aggregate gate, focused check
  names, supported systems, Playwright boundary, and separate live npm audit are
  described accurately in present tense.
- Keep the existing Rust, policy, coverage, evaluator, package, and x86_64-linux
  VM checks intact.

## Non-goals

- Do not add a legacy nixpkgs input or advertise any Darwin output.
- Do not add dependencies, change package versions or lockfiles, or perform the
  broad frontend/OpenAPI currency work from F-058.
- Do not add Playwright or live `npm audit` to the sandboxed aggregate; document
  their separate host/browser and live-advisory roles instead.
- Do not change either example's intended deployment behavior or the production
  NixOS module, wrapper chain, runtime policy, or security assertions.
- Do not make the documented focused integration command self-contained; F-027
  owns that runner.
- Do not claim that a green gate is a security proof, covers every supported
  browser, or resolves the browser-support half of U-003.

## Acceptance criteria

- Flake per-system outputs are generated for exactly `x86_64-linux` and
  `aarch64-linux`; evaluation exposes no Darwin package, check, or dev-shell
  output and needs no compatibility pin.
- `checks.<system>.web-build` builds the same committed SPA production artifact
  consumed by the Rust package, and `checks.<system>.web-test` runs the complete
  deterministic Vitest suite from the pinned npm closure.
- `checks.<system>.examples-nixos-eval` imports both shipped examples with the
  real underwrap flake/module/package and fails unless each complete
  `system.build.toplevel` evaluates successfully.
- The old shallow `examples-nix-parse` check and fake underwrap substitute are
  removed.
- Existing non-SPA checks retain their names and semantics, including the
  x86_64-linux-only VM check.
- README and testing documentation no longer call an inaccurately scoped gate
  complete; they list the deterministic aggregate coverage and keep Playwright
  plus live npm audit explicit and separate.
- Focused SPA/build/example checks, all-system evaluation, every non-coverage
  current-system Nix check, the production VM, and the ordinary direct SPA
  commands pass. Any unchanged external coverage-tool failure is recorded
  accurately and no check is weakened or claimed passing.

## Security boundary

- Protected asset: trustworthy release evidence for the privileged NixOS
  frontend and embedded admin SPA.
- Attacker: a defective or malicious repository change, or an invalid shipped
  example, that would otherwise cross from source into a falsely green release
  decision.
- Boundary direction: repository source and examples → deterministic aggregate
  checks → operator/release acceptance.
- Invariant: security mechanisms and assertions fail closed and are never
  removed or weakened merely to make a test pass; this task strengthens
  evidence only and does not claim a runtime security bypass is fixed.

## Validation

- `nix flake check --all-systems --no-build`
- `nix build .#checks.x86_64-linux.web-build .#checks.x86_64-linux.web-test .#checks.x86_64-linux.examples-nixos-eval`
- `nix build .#checks.x86_64-linux.workspace-test .#checks.x86_64-linux.workspace-nextest .#checks.x86_64-linux.workspace-clippy .#checks.x86_64-linux.workspace-fmt .#checks.x86_64-linux.workspace-audit .#checks.x86_64-linux.workspace-deny .#checks.x86_64-linux.frontend-backend-url-eval .#checks.x86_64-linux.frontend-swap-posture-eval`
- `nix build .#checks.x86_64-linux.vm-test`
- `cd web && npm test && npm run build`
- `nix flake check`
- `git diff --check`

## Dependencies

- D-182's approved Linux-only support matrix is required and already recorded.
- F-025 is complete; its pinned Node/npm/Playwright/nixpkgs state is the input
  baseline for these checks.
- F-027 and F-058 remain separate later findings.
