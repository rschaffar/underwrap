# Contributing

Underwrap is pre-release security software with a Linux backend and a privileged frontend supported only through its NixOS module.
Start with [Architecture](docs/architecture.md) and read [Security](docs/security.md) before changing or assessing a security boundary.
Report suspected vulnerabilities through [SECURITY.md](SECURITY.md), not a public issue containing sensitive details.

## Prepare a checkout

Use a Linux host with Nix and the `nix-command` and `flakes` features enabled.
The pinned development shell supports `x86_64-linux` and `aarch64-linux`.
Run these commands from the repository root in your working checkout:

```bash
nix develop
system=$(nix eval --impure --raw --expr builtins.currentSystem)
web_dist=$(nix build --no-link --print-out-paths ".#checks.${system}.web-build")
test -f "$web_dist/index.html"
export UNDERWRAP_WEB_DIST="$web_dist"
cargo build --locked -p underwrap-backend
cargo test --locked -p underwrap-protocol
```

The shell supplies tools, not a built SPA or packaged integration executables.
The backend's `build.rs` requires `index.html` in `UNDERWRAP_WEB_DIST`; without that variable, it looks in `web/dist`.
An empty placeholder bundle defeats this safeguard and is not a valid preparation step.
Rebuild the SPA artifact after frontend changes rather than embedding a stale bundle.

For daemon/backend integration, the supported runner prepares all three prerequisites itself:

```bash
./crates/integration-tests/run
```

It selects the current system's SPA, packaged daemon, and packaged backend, verifies their paths, and enters the pinned shell.
For raw workspace tests, use [the explicit prerequisite command](docs/testing.md#rust-and-integration-commands), not an unprepared `cargo test --workspace`.
Cargo builds do not install the setgid entry point, capability wrapper, dedicated identities, or namespace deployment.
Use the [NixOS tutorial](docs/getting-started.md) for deployment and the [VM check](docs/testing.md#deterministic-nix-gate) for the privileged chain.

## Work on the SPA

Inside `nix develop`, prepare the lock-selected packages and local generated types:

```bash
npm --prefix web ci
npm --prefix web run generate:api
npm --prefix web test
npm --prefix web run build
```

`npm test` and `npm run build` regenerate API types through their npm hooks.
The explicit generation step also prepares direct watch and coverage commands, which do not have those hooks.
`npm --prefix web run dev` starts Vite, not a backend.
Its `/v1` proxy is development tooling, not a deployment or production TLS-verification recipe; use the [prepared browser fixture](docs/testing.md#browser-and-live-advisory-checks) for complete authenticated UI tests.
Never point disposable test fixtures at production resources or accounts.

## Propose and review a change

Keep changes bounded and describe the behavior, rationale, risks, and checks in the contribution.
For nontrivial work needing continuity, choose one work owner, such as an issue, pull request, or agreed temporary working document.
There is no mandatory committed plan or completion archive.
Planning and read-only review do not authorize implementation, publication, or deployment.
Ask before adding dependencies, changing the `underwrap-exec → underwrap-ns` chain, or weakening a security boundary.

The repository default is a dedicated feature branch in a linked worktree based on current local `main`, unless already working in a suitable feature worktree.
Respect stricter user or global instructions, including required confirmation before mutating a primary checkout, and resolve conflicts before writing.
Preserve unrelated changes and keep commits reviewable.
After checks and feature commits, obtain approval for the exact verified feature tip and exact verified local `main` tip before an ordinary `--no-ff` merge.
If either tip changes, revalidate and obtain new approval.
A local merge does not authorize a push, release, history rewrite, branch deletion, worktree removal, or other cleanup.

Before committing, run the checks relevant to the changed boundary in [Testing](docs/testing.md#choose-checks-by-change).
State which checks ran, which were simulated, which were unavailable, and any remaining limitations.
A later pass does not explain an earlier failure; investigate rather than silently retrying until green.
Security-sensitive work needs human review of the affected end-to-end path, not only a passing unit test.
[AGENTS.md](AGENTS.md) supplies the mandatory cross-component review routing for coding agents.

## Engineering conventions

- Use Rust 2024 with the [pinned support policy](docs/testing.md#toolchain-and-dependency-policy), contextual `anyhow` errors, and typed `tracing` fields.
- Keep Rustfmt clean and Clippy warning-free.
- Limit `unsafe` to reviewed syscall/libc FFI and raw-fd ownership transitions, with explicit pointer, lifetime, ownership, or syscall invariants.
- Check privilege-changing syscall results and clear real, effective, and saved IDs on permanent drops.
- Fail closed instead of silently downgrading the selected security posture.
- Use `tempfile::NamedTempFile::persist()` or equivalent atomic replacement for persistent writes, without claiming transactions or power-loss durability from rename alone.
- Never weaken security assertions to make a test pass.
- Preserve meaningful protocol versions and fail-before-effect checks rather than adding unneeded compatibility paths before the first release.

Keep resource material in the storage and projection domains defined by [Security](docs/security.md).
Never copy secrets, credentials, tokens, resource blobs, key material, or secret-bearing fixtures into issues, reviews, commits, command lines, environment handoffs, or logs.
Log decision metadata only; identifiers can themselves contain sensitive operator input.
Generic errors must not format arbitrary source chains into responses or logs, and each new detailed diagnostic source needs its own material-flow review.
When discussing security, name the protected asset, attacker, and direction across the boundary.

For confirmation dialogs, preserve [request-error ownership](docs/testing.md#confirmation-error-ownership): expected request failures are handled outcomes, but invalidation and success-callback failures remain observable rejections.
Do not install generic or global rejection suppression to make UI tests quiet.

## Test and documentation layout

Rust tests live in sibling files, not inline production `mod tests { ... }` blocks:

```text
src/admin_api/csrf.rs
src/admin_api/csrf/tests.rs
```

Regular production files declare:

```rust
#[cfg(test)]
mod tests;
```

Crate roots use an explicit path to avoid `main.rs`/`lib.rs` collisions:

```rust
#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;
```

Put cross-module helpers in a crate-root-declared `#[cfg(test)]` `src/test_support.rs` and single-module helpers in that module's `tests.rs`.
Do not add root re-exports just to bypass privacy, and aim to keep each test file below roughly 600 lines.
Follow the [fixture and observation discipline](docs/testing.md#fixture-and-observation-discipline) when adding listeners, children, tracing, or fd witnesses.

Write one sentence per line in prose and keep one maintained explanation per topic.
Update the behavior's documentation owner instead of adding a permanent execution diary.
Keep `CLAUDE.md → AGENTS.md`, `.claude/skills → ../.agents/skills`, and `.pi/skills → ../.agents/skills` as symlinks to the canonical guidance, not vendor-specific copies.
Review [generated-contract and source-inclusion changes](docs/testing.md#generated-contracts-and-source-inclusion) alongside the implementation.
