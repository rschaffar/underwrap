# Pin the privileged release toolchain at Rust 1.97.1

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-024
Source themes: C-006

## Outcome

Every reproducible Rust build, development shell, check, coverage run, package, privileged Rust binary, and NixOS VM uses an explicit Rust **1.97.1** toolchain from a refreshed `rust-overlay` lock. This moves release artifacts beyond the known LLVM optimizer defect range affecting the previous Rust 1.94.1 pin while changing no product behavior and making no claim that underwrap was previously miscompiled.

Rust 1.97.1 is the smallest accepted fixed release from the review. The pin is explicit in `flake.nix`; future lock refreshes cannot silently change the compiler selected by `stable.latest`.

## Scope

- Replace `pkgs.rust-bin.stable.latest.default` with explicit `pkgs.rust-bin.stable."1.97.1".default`, preserving the existing `rust-src`, `rust-analyzer`, and `llvm-tools-preview` extensions and the single `craneLib.overrideToolchain` authority used by packages/checks/dev shell.
- Refresh **only** the `rust-overlay` input in `flake.lock` to a pinned revision that exposes Rust 1.97.1 for all supported systems:
  - `nixpkgs`, `crane`, `flake-utils`, `systems`, npm, Cargo dependencies, advisory database, and every other input remain byte-identical;
  - no broad `nix flake update`, dependency update, or F-025 advisory work is included.
- Add/retain deterministic version evidence:
  - `nix develop -c rustc --version` and `rustc -Vv` report exactly 1.97.1;
  - Cargo, Clippy, and rustfmt come from the same Rust 1.97.1 toolchain (Clippy reports its corresponding `0.1.97` component version);
  - the explicit-pin desired-state script passes;
  - package/check derivations evaluate with that explicit toolchain.
- Run every release boundary under the new Nix toolchain:
  - full Rust workspace tests, denied-warning clippy, rustfmt, cargo-nextest, cargo-audit, cargo-deny, and llvm coverage;
  - default workspace package plus the `underwrap-exec` and `underwrap-ns` release packages, and their deployed NixOS `security.wrappers`/capability paths through the VM;
  - integration tests and the production NixOS VM test;
  - SPA unit/build and auth/browser flows as the existing cross-language release evidence;
  - full `nix flake check`.
- If Rust 1.97.1 introduces a compile or denied-warning incompatibility, allow only the smallest behavior-preserving source/test syntax adaptation required by that compiler. Each such adaptation must be separately identified, reviewed, and validated; do not opportunistically modernize unrelated code.
- Synchronize `docs/testing.md` (and README only if needed) with the explicit compiler pin, version-verification command, affected-range/fixed-release rationale, and the evidence boundary: the update removes avoidable compiler uncertainty but neither proves a prior miscompilation nor adds a runtime guarantee.
- Preserve clean-HEAD evidence outside the repository:
  - desired-state script `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1230-rust-toolchain-red.py`, SHA-256 `ca2075e28bbde2fd8312bc9c49a7479f6744fac3ecff0d60a66c598878efc0e8`;
  - red log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1230-rust-toolchain-red.log`, SHA-256 `84621411b711b39cf83796887c2fa1f5be2641b5a3e6998f62bd26b226ca6fed`;
  - clean toolchain baseline `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1230-toolchain-baseline.log`, SHA-256 `8255a01d191f6e0a86ba0b7f2f6400d27b481fd965fdc4ba121c5a1877daac61`.
  The script exits 1 because the flake uses `stable.latest` rather than an explicit patch pin. The baseline proves rustc/cargo 1.94.1, Clippy 0.1.94, rustfmt 1.8.0, and LLVM 21.1.8.

Expected implementation/product paths are limited to:

- `flake.nix`
- `flake.lock`
- `docs/testing.md`
- optional minimal Rust/test files only if 1.97.1 itself requires behavior-preserving compatibility changes
- `README.md` only if its release-gate/version wording requires alignment

A narrower path set is preferred. Required board moves and `tickets/_handover/codebase_2026-07-18.md` updates are workflow state, not product-scope expansion. Mandatory fresh cleanup discovery runs first; no cleanup is pre-approved. Toolchain-induced source adaptations are fix units, not cleanup.

## Non-goals

- Do not update nixpkgs, crane, flake-utils, systems, Node/npm, Cargo dependencies/lock, advisory DB, Playwright, NixOS module options, or package behavior. F-025 and later release-gate findings remain separate.
- Do not use `stable.latest`, nightly, beta, a floating channel, or a version newer than 1.97.1 without a new explicit reason/decision. The accepted minimum fixed patch is the reproducible target.
- Do not claim underwrap was miscompiled, identify an exploit trigger without evidence, change unsafe/security assertions to appease the compiler, lower warning levels, disable optimization/LTO, weaken tests, skip privileged/VM/coverage gates, or retain the old compiler as fallback.
- Do not add dependencies, source refactors, edition/resolver/MSRV/rustdoc changes, or absorb F-025, F-026, F-027, F-056/F-057, any Opportunity, migration, or severity transition.

## Acceptance criteria

### Reproducible fixed toolchain

- `flake.nix` contains exactly one explicit Rust 1.97.1 selection; no `stable.latest`, second compiler authority, fallback, or old 1.94.1 production reference remains.
- `flake.lock` changes only the `rust-overlay.locked` revision/hash/timestamp needed for 1.97.1; every other node is byte-identical.
- `nix develop` reports rustc and Cargo 1.97.1, the corresponding Clippy 0.1.97 component, and retains rustfmt/rust-src/rust-analyzer/llvm-tools.

### Release evidence

- All workspace, integration, nextest, clippy, fmt, audit, deny, coverage, package, privileged-binary, SPA, browser, VM, and full-flake gates pass under the explicit toolchain, or an external/tool defect is recorded with exact evidence and independently shown unrelated without weakening a gate.
- Any compiler-required source adaptation is behavior-preserving, separately reviewed, and limited to the minimum path set.
- Documentation states only that artifacts are rebuilt/tested with the fixed compiler; it does not claim prior miscompilation or runtime exploitability.

### Scope

- No dependency/input other than rust-overlay, product behavior, protocol/config/API/Nix module, security assertion, optimization posture, later finding, or unrelated formatting change.
- F-024 is resolved. F-025+, release-gate improvements, rustdoc/resolver/MSRV, and every other finding/Opportunity remain separately queued.

## Security boundary

- Protected asset: correctness of compiled privilege, identity, namespace, policy, and secret-handling checks in deployed binaries.
- Actor/fault: no repository attacker trigger is established; the fault is avoidable uncertainty from a compiler version in a known fixed miscompilation range.
- Boundary direction: pinned rust-overlay/toolchain -> Crane packages/checks -> NixOS deployed privileged Rust binaries and backend/daemon services.
- Invariant: first-release artifacts and all validation evidence use the explicit fixed compiler without weakening source assertions or release gates.

## Validation

Run resource-intensive checks sequentially with `CARGO_BUILD_JOBS=2` and `PYTHONDONTWRITEBYTECODE=1`.

- Re-hash and preserve the red/baseline artifacts; run the desired-state script after the pin.
- Compare `flake.lock` structurally and byte-for-byte for every non-rust-overlay node.
- `nix develop -c bash -lc 'rustc --version; rustc -Vv; cargo --version; cargo clippy --version; rustfmt --version'`
- Inside `nix develop`: `cargo test --workspace`, `cargo fmt -- --check`, `cargo clippy --all-targets --workspace -- -D warnings`, `cargo nextest run --workspace`, `cargo audit`, and `cargo deny check bans sources`.
- `cd web && npm test && npm run build`; `nix develop -c bash -lc 'cd web && npm run test:e2e -- auth-flows.spec.ts'`; remove generated/browser output afterward.
- `nix build .#packages.x86_64-linux.default .#packages.x86_64-linux.underwrap-exec .#packages.x86_64-linux.underwrap-ns`
- Inspect privileged outputs with `file`, and verify package closure/binaries exist.
- `nix build .#checks.x86_64-linux.vm-test`
- `nix flake check`
- Static inventory: exact one version pin; lock-only rust-overlay node; no old compiler/floating channel; no dependency/source/module/API diff unless separately approved compatibility fix; no artifacts/processes.

## Dependencies

- The user-authorized F-015–F-024 batch includes this final item. No further ticket is authorized after its completion without the normal completion/continuation gate.
- Completed F-006–F-010 and F-019/F-020 cover privileged/runtime boundaries whose release artifacts must be rebuilt and retested.
- F-025 owns dependency advisory updates; F-026/F-027 own aggregate-gate/self-contained-runner improvements; F-056/F-057 own rustdoc/resolver/MSRV. All remain separate.
- No unresolved product decision. Cleanup discovery begins automatically under D-111 after materialization; no cleanup unit is pre-approved.
