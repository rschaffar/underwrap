# Prove every frontend security assertion with a negative matrix

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-030
Source themes: C-007

## Outcome

A fast real-NixOS evaluator proves every security-critical assertion currently
emitted by the production frontend module. Each admitted baseline builds its
`toplevel`; each invalid row reaches the intended assertion, produces the exact
sole failure message, and makes forced `system.build.toplevel` evaluation fail.
Removing, bypassing, or inverting an assertion can no longer remain hidden
behind valid-only examples or the large VM test.

## Scope

- Add `nix/tests/frontend-security-assertions-eval.nix` and expose it as the
  `frontend-security-assertions-eval` flake check on both supported Linux
  systems.
- Evaluate the real production module through `nixpkgs.lib.nixosSystem`; do not
  mock assertion expressions or duplicate production predicates in a test-only
  module.
- On one valid hardened instance/wrapper baseline:
  - locate exactly one assertion definition sourced from
    `nix/modules/underwrap.nix`;
  - bind an exact inventory of all 14 current assertion messages;
  - require every underwrap assertion and every global assertion to be true;
    and
  - force a successful `config.system.build.toplevel` evaluation.
- Add invalid rows for every assertion family and every independently meaningful
  predicate/name class inside combined assertions:
  - relative `targetRuntimeDirBase`, exact `/run/user`, and a `/run/user/...`
    descendant;
  - the distinct wrapper names `probe/w10544094` and `probe/w10571720`, whose
    SHA-256 prefixes both equal `ef4af79465f1`, to drive the target-UID collision
    assertion through production naming logic;
  - plaintext backend URL;
  - relative target, empty resource, no projection, and files paths that are
    empty, absolute, contain empty/`.`/`..` segments, duplicate, or overlap;
  - `LD_PRELOAD`, `LD_LIBRARY_PATH`, `LD_AUDIT`, and a generic `LD_*` name in
    both `envPassthrough` and `envSet`;
  - each forced passthrough name: `HOME`, `XDG_RUNTIME_DIR`,
    `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, and
    `XDG_STATE_HOME`;
  - `envSet.XDG_RUNTIME_DIR` and a relative value for each of `HOME` plus the
    four configurable base-XDG names;
  - reserved wrapper name `debug`;
  - representative plaintext ordinary swap and active zram swap writeback; and
  - a hardened kernel below 6.5 using pinned `linuxPackages_6_1`.
- For every invalid row, require the exact singleton false underwrap message,
  exactly one false assertion globally, and forced toplevel failure. Emit a
  reviewable `matrix.tsv` containing row, family, message, and toplevel result.
- Keep the existing backend-URL evaluator and rich 23-row swap/zram posture
  evaluator unchanged and passing; representative rows in the complete matrix
  complement rather than replace their preservation/configuration evidence.
- Update `docs/testing.md` with the exact complete-matrix evidence and limits.

## Non-goals

- Do not change, reorder, consolidate, weaken, or add a production assertion,
  Nix option/type, generated artifact, package/service definition, default,
  wrapper identity, target UID, environment policy, kernel floor, or swap
  posture.
- Do not add a dependency, production test hook, compatibility branch, or
  privileged/runtime behavior.
- Do not add type-invalid rows that fail before the existing assertions or use
  this ticket to validate additional names, paths, CA inputs, `accessPaths`, or
  generated-table shapes; those are F-031.
- Do not replace the deployed VM, claim runtime enforcement from evaluation
  evidence, or claim correctness for arbitrary NixOS/nixpkgs versions.
- Do not absorb F-031 input hardening, F-032 unsafe rationale, later findings,
  U-items, or any Opportunity.
- Do not remove the existing `frontend-backend-url-eval` or
  `frontend-swap-posture-eval` checks merely to avoid representative overlap.

## Acceptance criteria

- The new evaluator discovers exactly one production underwrap assertion
  definition and an exact 14-message inventory. A removed or newly untracked
  assertion fails that inventory check.
- The valid hardened baseline has all underwrap/global assertions true and a
  successfully forced toplevel.
- Every listed invalid row is option-type-valid, has exactly one false global
  assertion, has exactly the expected singleton false underwrap message, and
  has a failed forced toplevel.
- Every conjunct of the combined wrapper assertion and every enumerated
  security-sensitive environment name class has its own row, so one surviving
  predicate cannot mask another removed predicate behind the shared message.
- The collision row independently checks that its two distinct production input
  strings share the expected 12-hex prefix before requiring only the target-UID
  collision assertion to fail.
- The old-kernel row uses the evaluated kernel version in the exact dynamic
  message and fails only the Linux 6.5 assertion.
- Plain HTTP, ordinary-swap, and zram-writeback rows agree with the existing
  focused checks; those focused checks retain their current valid/configuration
  evidence and names.
- The matrix artifact is deterministic and names every row/family without
  embedding secrets or machine-specific paths.
- Both supported-system outputs evaluate; the x86_64 check builds; the complete
  aggregate retains every prior check.
- Only the new evaluator, flake check wiring, and testing documentation change.
  No module, dependency/lock, product behavior, F-031+, U-item, or Opportunity
  change is introduced.

## Security boundary

- Protected asset: brokered resource confidentiality/integrity and the
  privilege, identity, environment, path, kernel, and paging preconditions of
  the NixOS frontend boundary.
- Attacker: an untrusted local caller who could benefit if a regression or
  trusted-operator mistake made an unsafe root configuration deployable; the
  caller does not control NixOS configuration directly.
- Boundary direction: trusted root NixOS configuration → generated privileged
  frontend identities/services/wrappers → untrusted caller and trusted target.
- Invariant: security mechanisms fail closed. Existing module assertions must
  reject unsafe configuration during evaluation; this ticket tests that
  boundary without changing it.

## Validation

- Preserve the red/static baseline: the production module contributes 14
  assertion families, but only backend URL plus ordinary-swap/zram have focused
  negative evaluators; the desired-evidence inventory currently exits 1 with
  the complete evaluator/check/docs conditions absent.
- Re-run the deterministic desired-evidence inventory and require all conditions
  to pass.
- Re-run the independent collision probe and require distinct inputs,
  `ef4af79465f1` for both prefixes, the exact collision message, and failed
  toplevel.
- `nix build --no-link .#checks.x86_64-linux.frontend-security-assertions-eval`
- `nix build --no-link .#checks.x86_64-linux.frontend-backend-url-eval`
- `nix build --no-link .#checks.x86_64-linux.frontend-swap-posture-eval`
- `nix flake check --all-systems --no-build`
- `nix flake check`
- `git diff --check`
- Static inventory: exact 14 production messages, complete row/name-class set,
  one real-module evaluation path, unchanged old focused checks, no module/
  dependency/lock/product change, generated output, or retained test process.

## Dependencies

- F-004's `frontend-backend-url-eval` remains the detailed HTTPS/base-path
  authority and is not replaced.
- F-010's `frontend-swap-posture-eval` remains the detailed ordinary-swap/zram
  authority and is not replaced.
- F-028's Linux 6.5 hardened-path evidence remains unchanged; this task only
  proves the configured-kernel assertion fails on `linuxPackages_6_1`.
- F-029 is complete and automatically accepted at
  `cf7fd3937ded52de2d28db45f5d659e492501dcf`.
- D-174 authorizes one-ticket-at-a-time continuation subject to all stop,
  cleanup, review, validation, and transition rules.
- F-031 and later findings remain separate.
