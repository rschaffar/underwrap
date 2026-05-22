# Task: Slim `web.rs` SPA hosting; drop runtime disk-mode override

Parent: `1700_story_backend_framework_migration.md`
Depends on: `1710_task_port_admin_api_to_axum_router.md`

## Background

`crates/backend-backendd/src/web.rs` was 218 lines of hand-rolled
SPA hosting:

- `WebAssetSource::Embedded` / `WebAssetSource::Disk` chose between
  the baked-in `rust_embed::Embed` table and a directory pointed at
  by `UNDERWRAP_BACKENDD_WEB_DIR`.
- Hand-rolled `normalise_rel_path`, `is_inside`, `is_hashed_asset`,
  `content_type_for`, `cache_control_for`, `is_spa_request`,
  `serve_web_request`, `spa_not_available_response`.

## Research outcome

We considered three library replacements:

1. **`axum-embed`** — stale (single 0.1.0 from Dec 2023, single
   maintainer, pins `axum-core 0.4` while the workspace is on
   `axum 0.8` / `axum-core 0.5`). Disqualified for a security-
   sensitive admin listener.
2. **`memory-serve`** — actively maintained, ETag + brotli + cache,
   but still a single-maintainer crate adding routing logic and
   transitive surface onto the admin listener.
3. **A tiny `rust_embed`-backed handler in this crate** — `rust_embed`
   is widely used and we already depend on it; the handler itself
   is ~50 lines, trivially auditable, no new transitive surface.

We picked (3).

We also reconsidered the disk-vs-embedded split. In debug builds
`rust_embed` already reads dynamically from the path captured at
compile time via `$UNDERWRAP_WEB_DIST` — and `build.rs` defaults
that to `<workspace>/web/dist`. So the runtime
`UNDERWRAP_BACKENDD_WEB_DIR` override is redundant:

- **Production**: Nix bakes the SPA into the binary closure.
  Operator cannot swap the SPA without swapping the daemon binary
  (a real defence-in-depth property of the admin listener).
- **Dev / e2e**: `rust_embed`'s debug-mode disk reading covers
  iterating on the SPA without rebuilding the daemon.

We dropped disk mode entirely.

## What changed

- `web.rs` rewritten as a single async handler `serve_spa(req)`
  that walks `EmbeddedAssets::get`, falls back to `index.html` for
  unknown paths, sets `Content-Type` via `mime_guess`, and applies
  the three-tier `Cache-Control` rule (`no-store` for `index.html`,
  `immutable` for `assets/*`, short cache otherwise). HEAD requests
  return headers + empty body; non-GET/HEAD methods return 405.
  Total ~50 lines.
- `WebAssetSource`, `UNDERWRAP_BACKENDD_WEB_DIR`, the `web_source`
  field on `AdminRuntimeState`, and all the per-helper unit tests
  removed.
- `http/mod.rs` installs `serve_spa` directly as the top-level
  `Router::fallback` when the admin UI is enabled. The
  `spa_fallback_handler` indirection is gone.
- `web/tests.rs` rewritten as end-to-end tests against `serve_spa`:
  root → `index.html` + `no-store`, SPA history fallback, missing
  asset path → fallback, path-traversal → fallback, hashed asset
  → `immutable`, HEAD body empty, POST → 405.
- e2e harness (`web/e2e/backendd.ts`) no longer sets
  `UNDERWRAP_BACKENDD_WEB_DIR` or writes a stray `session_key_file`
  config field.
- `mime_guess` stays (still used by `serve_spa`). No new
  dependencies, no new `tower-http` features.

## Acceptance criteria

- [x] `crates/backend-backendd/src/web.rs` is ~50 lines, no
      `WebAssetSource`, no `UNDERWRAP_BACKENDD_WEB_DIR`.
- [x] `cargo test -p backend-backendd --lib web::` passes (7 tests).
- [x] `cargo test --workspace` passes.
- [x] `cargo fmt --all` and
      `cargo clippy --all-targets --workspace -- -D warnings` clean.
- [x] `cd web && npm test` passes (65 tests).
- [x] `cd web && npm run build` succeeds.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.
