# Task: Compile-time check that `web/dist/` is bundled into the backend

Resolves: B-COR-16 (deferred from `1360_task_doc_comment_sweep`).

## Problem

`crates/backend-backendd/src/web.rs` ships an `spa_unavailable()`
fallback that serves a developer-instruction HTML page with HTTP 503
when the embedded `web/dist/` is empty (e.g. someone built the
backend without running `npm run build` first). The Nix flake
closure guarantees the bundle is present for release artifacts, so
release-quality binaries should never hit this path — a `cargo
build` without the bundle should fail fast at build time instead of
producing a binary that 503s on `GET /`.

## What to do

1. Add `crates/backend-backendd/build.rs` that reads the
   `UNDERWRAP_WEB_DIST` build-time env var (path to the populated
   `web/dist/` directory) and:
   - asserts the directory exists and contains `index.html`;
   - emits `cargo:rustc-env=UNDERWRAP_WEB_DIST=<path>` so
     `rust-embed` can pick it up at compile time;
   - emits `cargo:rerun-if-env-changed=UNDERWRAP_WEB_DIST` and
     `cargo:rerun-if-changed=<path>/index.html`.

2. Switch `web.rs` to embed from the env-var-resolved path. With
   `rust-embed` ≥ 8 this is `#[folder = "$UNDERWRAP_WEB_DIST"]`.
   Drop the `spa_unavailable()` 503 fallback and the matching
   route arm — the path is unreachable once the build asserts the
   bundle.

3. Wire the env var through the Nix flake's
   `backend-backendd` derivation so `nix build` always provides
   the populated bundle path. The flake already builds the SPA
   into `web/dist/` (or a Nix-store equivalent); this just exposes
   that path to the Rust build via the env var.

4. For the development shell, document the workflow in
   `crates/backend-backendd/README.md` (or `docs/backend.md` if
   that's the canonical pointer): `npm --prefix web run build`
   once, then `UNDERWRAP_WEB_DIST=$PWD/web/dist cargo build -p
   backend-backendd`. Optional: have `nix develop` export the env
   var pointing at the SPA bundle so `cargo build` works without
   manual setup.

## Notes

- The change must keep the SPA-asset fallback route (the catch-all
  that serves `index.html` for client-side router paths). Only the
  503 fallback for an empty bundle goes away.
- No runtime behaviour change for release binaries — those already
  carry a populated bundle.
- This is greenfield, so no compatibility shim for the old
  `web/dist`-relative `#[folder = "../../web/dist"]` form is
  needed; the env-var form is the only path going forward.

## Acceptance

- `cargo build -p backend-backendd` **without** `UNDERWRAP_WEB_DIST`
  set (and without an existing bundle on disk) fails at build time
  with a clear error message pointing at the env var.
- `UNDERWRAP_WEB_DIST=$PWD/web/dist cargo build -p backend-backendd`
  (after `npm --prefix web run build`) produces a working binary
  that serves the SPA on `GET /`.
- `nix build .#backend-backendd` continues to produce a working
  binary.
- `nix build .#checks.x86_64-linux.vm-test` stays green.
- `spa_unavailable()` and the matching route arm are gone from
  `web.rs`.
