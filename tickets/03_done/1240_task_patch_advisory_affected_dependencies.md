# Patch advisory-affected build and runtime dependencies

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-025
Source themes: C-006

## Outcome

The reproducible Rust, npm, Node, and advisory inputs use the smallest reviewed supported-line patches that remove every fix-available advisory reproduced for F-025. The Nix development/build environment supplies security-fixed Node 24.19.0, the npm lock reports zero known vulnerabilities against the live registry snapshot, and the pinned RustSec gate reports zero vulnerabilities/unsound warnings with fixes available.

This is dependency and build-input maintenance, not evidence that the deployed application was exploitable. The admin UI is served as static assets rather than through Vite/Vitest development servers. The newly reproduced `h2` advisory is in backend/client runtime dependency paths, but no underwrap-specific trigger or demonstrated denial of service is claimed.

## Scope

- Update only these Rust lockfile lines to their first reviewed patched releases:
  - `anyhow` 1.0.102 → 1.0.103 (`RUSTSEC-2026-0190`);
  - `h2` 0.4.14 → 0.4.16 (`RUSTSEC-2026-0258`);
  - `event-listener` 5.4.1 → 5.4.2 (`RUSTSEC-2026-0221`);
  - `rand` 0.8.5 → 0.8.6 and 0.9.2 → 0.9.3 (`RUSTSEC-2026-0097`).
  Use Cargo 1.97.1 exact `cargo update -p ... --precise ...` operations. Permit only the resulting checksum/reference changes and Cargo's deterministic re-resolution of references among the already locked `windows-sys` 0.52/0.59/0.60/0.61 lines; no `windows-sys` package version is added or removed. Do not change `Cargo.toml`, Rust source, features, or major/minor lines.
- Refresh the pinned RustSec advisory database in `flake.nix` from commit `b5fc89b8be99e96f79194d8a6f11e9b4143b99f0` to reviewed current commit `2f08fbb85332687b721f2f22706d07448369451b` and its unpacked hash `sha256-hRoCd9955xmj8kS1ujgkv7Xg2IjcQt/WyYBow3DXZIc=`. The refreshed database must make the old lock fail for `h2` and the patched lock pass with only the two no-fix unmaintained notices below.
- Retain `paste` 1.0.15 (`RUSTSEC-2024-0436`) and `rustls-pemfile` 2.2.0 (`RUSTSEC-2025-0134`) as the only allowed informational warnings. Neither advisory has a patched release. Replacing either crate would require source/API/dependency restructuring and is not a fix-available supported-line patch; document the residual maintenance notices without calling them vulnerabilities fixed by this task.
- Refresh only the `nixpkgs.locked` revision/hash/timestamp in `flake.lock` from `8d029aa64915e54b7846873d9583af4c9fd21ea4` to reviewed revision `afe3d8ac4395617bdcdac9f188ac8717a062e014`. This supplies Node 24.19.0/npm 11.17.0, beyond the official Node 24.18.1 July 2026 security release, while preserving:
  - explicit Rust 1.97.1 and its components from F-024;
  - the `rust-overlay`, `crane`, `flake-utils`, and `systems` locked nodes byte-for-byte;
  - all flake output/module/check definitions except the two hashes and advisory snapshot named here.
- Run `npm audit fix --package-lock-only --ignore-scripts` inside the updated Nix development shell. The resulting lock-only supported-line updates must remove the currently reproduced ten npm vulnerability records (2 critical, 6 high, 1 moderate, 1 low), including the affected `@babel/core`, `@redocly/openapi-core`/`js-yaml`, `@vitest/coverage-v8`/`vitest`, `brace-expansion`, `nanoid`, `postcss`, `vite`, and `ws` chains.
  - Keep all existing direct dependency major lines and do not run a general `npm update`, `npm audit fix --force`, or F-058 currency refresh.
  - Allow only dependency closure changes required by npm's audit fix, including their Babel/Vitest/browser-data helpers and lock representation.
- Align the direct `@playwright/test` range and lock to **1.61.1**, exactly matching the updated nixpkgs `playwright-driver` 1.61.1/browser revision 1228. This is a required compatibility adaptation to the nixpkgs refresh: leaving 1.58.2 expects missing browser revision 1208; floating to 1.62.1 expects missing revision 1234; exact 1.61.1 launches the Nix browser and passes the focused real-browser flow. This advances but does not resolve F-058.
- Update only the resulting `npmDepsHash` in `flake.nix` to `sha256-LHeYCM1f2AjGzQtnaCFsxc0fIWPkCPdDQ6Lm1HgcAx0=` and prove the Nix `underwrap-web-dist` build consumes the audited lock.
- Synchronize `docs/testing.md` with the current dependency evidence:
  - name `npm audit --package-lock-only` as a separate required lock audit (not yet an aggregate flake check; F-026 remains separate);
  - state that the pinned RustSec snapshot has zero fix-available findings and two explicit unmaintained/no-patch notices;
  - state that Node is build/development tooling and does not enter the embedded static backend closure;
  - explain that Playwright package/driver versions move together with nixpkgs;
  - preserve the existing statement that test/audit success does not establish absence of dependency vulnerabilities.
- Repair exactly two pre-existing `web/e2e/wrappers.spec.ts` fixture setups exposed by the required full-browser gate after F-014:
  - clean HEAD `c8941506e51d3d28a6a41077ac0317f0077b465c` with the old dependency/browser locks fails both deterministically with `404 unknown client instance`; baseline log SHA-256 `4a24e872fad2f7d2fb41102c6611a3c2390133471872e6c17da50093c0e235a9`;
  - create one real client instance in each affected isolated backend through the authenticated admin API, using that backend's generated public `cert.pem`, and use the returned client id rather than synthetic `dev-laptop` in permission URLs/test ids;
  - retain `seedPair(...)` in the cell-editor test and use it instead of resource-only creation in the raw-policy test, because F-014 requires the exact wrapper/resource pair before permission access;
  - do not modify the backend, application UI/API client, shared backend fixture, F-014 validation order, or any other E2E scenario. Record this as restoring stale F-014 browser evidence, not as an F-025 product behavior change.
- Preserve clean-HEAD evidence outside the repository:
  - desired-state script `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1240-dependency-advisory-red.py`, SHA-256 `5b2a0a604ce6546a28c4160dfda4d40923efb6e601a3e8cef902753ca10b69c2`;
  - red log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1240-dependency-advisory-red.log`, SHA-256 `af5ca1fbab7633c692d8f4b10b169bd99c02918b67aaca67d9b7960d282a2d1b`;
  - current npm audit JSON, SHA-256 `9f7885dc44681adf6c76c4c1a72a5aedd70fa8ef6b94d40e72f8e8c2ca26361c`, reporting ten vulnerable package records;
  - current live RustSec audit JSON, SHA-256 `281852ede19eae20a8db550715fa8bdc1ad8008766e6f24f982d1097e803cbb9`, reporting one `h2` vulnerability, four fix-available unsound warnings across `anyhow`, `event-listener`, and two `rand` lines, plus two unmaintained/no-patch notices;
  - exact dependency-path baseline `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1240-rust-affected-tree.log`, SHA-256 `69a3984f30d9af0910ea58439b7dea75875bb78515dcac584c07c159ba2fc19a`;
  - updated-tree feasibility proof `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1240-dependency-advisory-green-probe.log`, SHA-256 `9657ebb6b1586b196b7b0d60e5f90465a8300e1c0ec9593b78633944d02adda6`.

Expected implementation/product paths are limited to:

- `Cargo.lock`
- `flake.lock`
- `flake.nix`
- `web/package.json`
- `web/package-lock.json`
- `docs/testing.md`
- `web/e2e/wrappers.spec.ts` (only the exact pre-existing F-014 fixture repair above)

Required board moves and `tickets/_handover/codebase_2026-07-18.md` updates are workflow state, not product-scope expansion. Mandatory fresh cleanup discovery runs first; no cleanup is pre-approved. Dependency/input lock changes are the F-025 fix, not cleanup.

## Non-goals

- Do not add a dependency, change any Rust/application-TypeScript/Python/Nix module/runtime source, change protocol/API/config/storage behavior, or alter the `underwrap-exec → underwrap-ns` chain. The exact `web/e2e/wrappers.spec.ts` fixture repair above is the sole test-source exception.
- Do not update Rust beyond 1.97.1 or change F-024's `rust-overlay` node, Crane authority, components, optimizer posture, warnings, or coverage gate.
- Do not update `crane`, `rust-overlay`, `flake-utils`, or `systems`; do not perform a broad `nix flake update`.
- Do not run broad `cargo update`, `npm update`, `npm audit fix --force`, major-version migrations, or remove direct dependencies. Do not absorb F-058 frontend/OpenAPI currency.
- Do not migrate away from `paste` or `rustls-pemfile`, suppress/ignore a newly fix-available advisory, claim the two maintenance notices are resolved, or weaken audit policy.
- Do not add npm audit/SPA/example evaluation to `nix flake check`, rename the aggregate gate, or change example semantics; F-026 remains separate.
- Do not build a self-contained integration runner or alter binary discovery; F-027 remains separate.
- Do not claim current scans prove future or complete dependency safety, that the old dependencies were exploited, that the deployed static SPA exposed Vite/Vitest servers, or that no `h2` runtime trigger exists.
- Do not absorb any later finding, Opportunity, unresolved item, migration, or severity transition.

## Acceptance criteria

### Exact supported-line patches

- `Cargo.lock` contains exactly `anyhow` 1.0.103, `h2` 0.4.16, `event-listener` 5.4.2, `rand` 0.8.6 and 0.9.3, with no affected older instance; manifests/source are unchanged.
- The reviewed RustSec snapshot reports zero vulnerabilities and no fix-available unsound warnings; exactly the two named unmaintained/no-patch notices remain visible and allowed.
- `npm audit --package-lock-only --json` reports zero vulnerabilities. `web/package.json` changes only `@playwright/test` `^1.58.2` → `^1.61.1`; every other direct range and all application source remain unchanged.
- Nix supplies Node 24.19.0/npm 11.17.0 and Playwright driver 1.61.1; npm locks Playwright 1.61.1. The package launches the Nix browser and the real-browser suite passes.
- `flake.lock` changes only the three `nixpkgs.locked` fields. `flake.nix` changes only the advisory URL/hash and `npmDepsHash`. Explicit Rust 1.97.1 and every other input remain exact.

### Evidence and behavior preservation

- Desired-state 20-failure script becomes fully green. The Nix web-dist/default packages, Rust workspace, SPA tests/build, full browser suite, integration coverage, and production VM pass under the updated Nix closure.
- The two corrected wrappers browser tests create real client/resource/pair prerequisites and pass; clean-HEAD evidence proves their former failure predates the F-025 dependency update. No backend assertion or reference gate is weakened.
- No source, API, config, protocol, security assertion, dependency feature, major line, or runtime behavior changes.
- Documentation distinguishes current lock-snapshot evidence, residual no-patch maintenance notices, developer/build exposure, static deployed assets, and the absence of exploitability claims.

### Scope

- F-025 is resolved. F-026/F-027 aggregate/runner work and F-058 broad frontend/OpenAPI currency remain separately queued; F-058 records only the required Playwright compatibility advance.
- No unrelated lockfile currency, dependency addition/removal, later finding, Opportunity, or unresolved item.

## Security boundary

- Protected assets: developer/build-host files and availability, build integrity, and runtime HTTP/2 availability in binaries that include `h2`.
- Actor/fault: advisory-specific malicious development inputs/network traffic or runtime HTTP/2 frames reach affected dependency code; exact underwrap triggers are not established.
- Boundary direction: pinned Cargo/npm/nixpkgs/advisory inputs → development and reproducible build tools → embedded static SPA and Rust package/VM closures.
- Invariant: every reproduced fix-available advisory is removed by a supported-line patch without weakening tests, audits, compiler authority, or runtime security mechanisms; residual no-patch notices remain explicit.

## Validation

Run resource-intensive checks sequentially with `CARGO_BUILD_JOBS=2` and `PYTHONDONTWRITEBYTECODE=1`.

- Re-hash/run the desired-state script before and after the fix; structurally assert the exact allowed Cargo/npm/Nix/flake diffs and no source changes.
- `nix develop -c bash -lc 'node --version; npm --version; rustc --version; cargo --version; cargo clippy --version; rustfmt --version'`
- `nix develop -c cargo audit --json` and `nix build .#checks.x86_64-linux.workspace-audit -L`; assert zero vulnerabilities/fix-available warnings and exactly the two residual unmaintained notices.
- `nix develop -c bash -lc 'cd web && npm ci --ignore-scripts && npm audit --package-lock-only --json && npm test && npm run build'`
- Prove nixpkgs `playwright-driver` is 1.61.1, package/lock are 1.61.1, and browser revision 1228 is present; run `cd web && npm run test:e2e` inside `nix develop` with the exact built backend.
- Inside `nix develop`: `cargo test --workspace`, `cargo fmt -- --check`, `cargo clippy --all-targets --workspace -- -D warnings`, `cargo nextest run --workspace`, `cargo deny check bans sources`.
- `nix build .#packages.x86_64-linux.underwrap-web-dist .#packages.x86_64-linux.default .#packages.x86_64-linux.underwrap-exec .#packages.x86_64-linux.underwrap-ns`
- `nix build .#checks.x86_64-linux.vm-test`
- `nix flake check`; preserve exact output if the independently reproduced external LLVM 22 `llvm-profdata merge` SIGILL recurs only after all coverage tests pass.
- Static inventory: exact dependency/version/advisory/input/hash allowlists; no generated schema/dist/browser/Python artifacts, worktrees, test services, QEMU, or VDE processes.

## Dependencies

- F-024 is complete and accepted at `d817783e27b3f11d661051e4ccbff1ddbe24c5eb`; explicit Rust 1.97.1 must remain unchanged.
- D-174 authorizes one-ticket-at-a-time execution through all remaining accepted findings without routine ticket/completion prompts, subject to every stop condition.
- F-026 owns aggregate npm/SPA/example gates; F-027 owns a self-contained integration runner; F-058 owns broad frontend/OpenAPI currency. The required Playwright 1.61.1 alignment is recorded as an F-058 advance, not resolution.
- No unresolved product decision and no new dependency. Cleanup discovery begins automatically after reviewed materialization.
