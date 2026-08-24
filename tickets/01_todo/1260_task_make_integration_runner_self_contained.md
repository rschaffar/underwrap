# Make the focused integration test runner self-contained

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-027
Source themes: C-006

## Outcome

One documented repository runner prepares the committed SPA, supplies the exact
backend and daemon executables, and runs the complete integration-test crate
from a clean checkout without relying on stale `web/dist`, workspace `target/`
artifacts, or unrelated binaries on `PATH`.

## Scope

- Add one executable runner owned by `crates/integration-tests/` that:
  - resolves the repository root and current supported Linux system;
  - obtains the pinned SPA artifact and packaged workspace binaries from the
    flake;
  - enters the pinned development environment; and
  - runs `cargo test -p integration-tests`, forwarding caller arguments and
    preserving an explicit `CARGO_TARGET_DIR` when supplied.
- Make integration-test process fixtures accept only the exact existing
  `CARGO_BIN_EXE_underwrapd` and `CARGO_BIN_EXE_underwrap-backend` paths supplied
  by the supported runner/Nix checks. Remove workspace-target and `PATH`
  fallbacks plus stale graceful-skip wording so missing prerequisites fail
  immediately and actionably.
- Keep the existing Nix workspace test/nextest wiring as the canonical source of
  packaged binary paths.
- Update `docs/testing.md`, `AGENTS.md`, and the relevant flake comments so the
  focused integration command and its deterministic prerequisites are accurate.

## Non-goals

- Do not add a dependency, lockfile input, task-runner framework, CI service, or
  general repository command dispatcher.
- Do not add another aggregate flake check or rerun the integration crate a third
  time inside `nix flake check`; F-026's aggregate coverage remains unchanged.
- Do not change integration scenarios, production Rust behavior, HTTP/API/wire
  contracts, runtime policy, test parallelism, or capability-dependent skip
  behavior.
- Do not change the `underwrap-exec → underwrap-ns` chain, NixOS module, VM test,
  package contents, browser suite, or security assertions.
- Do not absorb F-028 or later evidence findings, F-050 fixture cleanup, F-058
  currency work, any Opportunity, or the browser-support half of U-003.
- Do not describe the runner as offline, hermetic beyond the flake/Cargo input
  model, or evidence that integration tests are a security proof.

## Acceptance criteria

- From a clean Git archive with no `web/dist`, workspace `target/`, or installed
  underwrap binaries, the supported runner builds/resolves its prerequisites and
  the complete integration-test crate passes with a fresh `CARGO_TARGET_DIR`.
- The runner uses the current system's flake `web-build` artifact and exact
  packaged workspace `underwrapd`/`underwrap-backend` binaries, then invokes the
  pinned Rust/Cargo environment without placing secret material in argv or the
  environment.
- Runner arguments are forwarded losslessly after the fixed
  `cargo test -p integration-tests` selector, and an operator-supplied
  `CARGO_TARGET_DIR` is retained.
- Real-process tests no longer search repository `target/{debug,release}` or
  `PATH`; an absent, relative, nonexistent, non-file/non-executable, or
  incorrectly named prerequisite path fails with a clear instruction to use the
  supported runner. The runner itself supplies the exact Nix-built paths; the
  resolver need not prove the provenance of an arbitrary same-named executable.
- Existing in-process integration tests and all four real-process tests execute;
  none silently skips because a binary or SPA artifact is unavailable.
- Nix workspace-test/nextest continue to supply exact packaged binaries and pass
  with their existing bounded test parallelism.
- Published and contributor testing instructions name the supported runner
  instead of the non-self-contained raw Cargo command and retain the distinct
  host-capability limitation.
- No dependency/lock, product source outside the integration-test resolver,
  aggregate-check set, test scenario, NixOS/privileged path, or later finding is
  changed.

## Security boundary

- Protected asset: trustworthy mTLS, policy, persistence, and daemon/backend
  integration evidence used for release decisions.
- Attacker/fault: a defective repository change could otherwise cross from
  source into a falsely green or partially skipped focused test result through
  stale local build artifacts.
- Boundary direction: clean repository source → pinned build prerequisites →
  exact real-process integration execution → release evidence.
- Invariant: missing evidence fails closed and no security assertion or test is
  removed or weakened to make the runner pass; the runner carries paths only,
  never credentials or secret values.

## Validation

- Reproduce the clean-archive red baseline without the runner: missing SPA first,
  then four missing exact-binary failures after supplying only the SPA.
- In a fresh clean archive with a fresh target directory:
  `CARGO_TARGET_DIR=<fresh> ./crates/integration-tests/run`
- Verify the complete integration result count and all four real-process test
  names; verify no skip message for missing SPA/backend/daemon prerequisites.
- `nix build --no-link .#checks.x86_64-linux.workspace-test .#checks.x86_64-linux.workspace-nextest`
- `cargo fmt -- --check`
- `cargo clippy --all-targets -p integration-tests -- -D warnings`
- `nix flake check --all-systems --no-build`
- `nix flake check`
- `bash -n crates/integration-tests/run`
- `git diff --check`
- Static inventory: exact changed paths and binary environment variables; no
  target/PATH fallback, stale raw command, lockfile diff, generated SPA/schema,
  test service, or process residue.

## Dependencies

- F-026 is complete and automatically accepted at
  `4e1981958ae4d5ba7d41193d165f81f475a63761`; its `web-build` check and explicit
  supported-system matrix are runner inputs and must remain unchanged.
- D-174 authorizes one-ticket-at-a-time continuation subject to all stop,
  cleanup, independent-review, validation, and severity-transition rules.
- F-028 and later findings remain separate.
