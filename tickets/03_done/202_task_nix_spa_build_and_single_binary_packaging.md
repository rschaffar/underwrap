# Task: Build the SPA in Nix and bundle it into the backendd binary

Parent: `200_story_backendd_serves_spa_and_admin_tls.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Add a Nix derivation that builds the SPA (`web/`) using `npm ci && npm run build` (or `pnpm`) producing `dist/`.
- Pass the built `dist/` into the Rust build of `backend-backendd` as an environment variable (e.g. `UNDERWRAP_WEB_DIST=/nix/store/...-web-dist`) that the `rust-embed` derive in ticket 201 honors at compile time.
- Resulting Nix package: a single binary closure containing both the Rust binary and the embedded SPA. No `nodejs` in the runtime closure.
- Local `cargo build -p backend-backendd` continues to work; in dev mode the binary expects `UNDERWRAP_BACKENDD_WEB_DIR` to be set at runtime for asset serving (no embed).
- Ensure new `web/` files are git-tracked before `nix build` runs (Nix flake source snapshot gotcha — see `.pi/skills/underwrap-ticket-workflow/SKILL.md`).

## Key files likely involved

- `flake.nix`
- `crates/backend-backendd/Cargo.toml`, optional `build.rs`

## Acceptance criteria

- [ ] `nix build .#default` produces a single binary whose runtime closure does not contain `nodejs`.
- [ ] The binary serves the SPA out of the box (no external files needed).
- [ ] Local `cargo build -p backend-backendd` continues to work in dev disk mode.
- [ ] A `nix flake check` step verifies that the closure does not pull `nodejs` for the default package.
- [ ] No regressions on the VM test.
