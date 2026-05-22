# Task: Replace `web.rs` SPA hosting with `axum-embed` + `tower-http::ServeDir`

Parent: `1700_story_backend_framework_migration.md`
Depends on: `1710_task_port_admin_api_to_axum_router.md`

## Background

`crates/backend-backendd/src/web.rs` is 218 lines of hand-rolled
SPA hosting:

- `WebAssetSource::Embedded` reads from `rust_embed::Embed`
  baked-in assets; `WebAssetSource::Disk` reads from the dev
  directory pointed at by `UNDERWRAP_BACKENDD_WEB_DIR`.
- `normalise_rel_path` strips leading slash, rejects `..` and
  `.` segments.
- `is_inside` is a defence-in-depth prefix check for the disk
  source.
- `is_hashed_asset`, `content_type_for`, `cache_control_for` set
  per-file MIME and cache rules. `index.html` is `no-store`,
  `assets/*` is `public, max-age=31536000, immutable`, everything
  else gets a short cache.
- `serve_web_request` performs the SPA history fallback to
  `index.html` when the requested path doesn't resolve.
- No `ETag` / `If-None-Match` handling, no `Last-Modified`,
  no precompressed-asset negotiation.

The library replacements:

- [`axum-embed`](https://docs.rs/axum-embed) wraps a
  `rust_embed::Embed` type as a Tower service. Handles MIME,
  ETag/304, Cache-Control, history fallback, gzip/br
  precompression.
- [`tower-http::services::ServeDir`](https://docs.rs/tower-http/latest/tower_http/services/struct.ServeDir.html)
  + `ServeFile` covers the disk source with proper HTTP semantics
  and `.fallback(ServeFile::new(...))` for SPA history fallback.

## Scope

- Add dependencies to `crates/backend-backendd/Cargo.toml`:
  - `axum-embed` (current stable release).
  - `tower-http` with the `fs` feature (if not already enabled
    transitively by 1710).
- Keep `rust_embed::Embed` declaration for `EmbeddedAssets` as it
  is today (`#[folder = "$UNDERWRAP_WEB_DIST"]`).
- Replace the entirety of `web.rs` with:
  - A small `web` module exporting a `pub fn spa_service(...)`
    that returns a `Tower` service ready to use with
    `Router::fallback_service(...)`.
  - The function reads `UNDERWRAP_BACKENDD_WEB_DIR` exactly as
    today (`WebAssetSource::from_env`-equivalent). If set and
    non-empty, return `ServeDir::new(path).not_found_service(ServeFile::new(path.join("index.html")))`.
    Otherwise return `ServeEmbed::<EmbeddedAssets>::with_parameters(...)`
    configured with `index.html` as the SPA fallback.
- Path-traversal defence: `ServeDir` rejects `..` paths by
  default; verify this with a unit test that requests
  `/../etc/passwd` (or similar) and asserts on a 404 response.
  Likewise for the embedded source via `axum-embed`.
- MIME types: rely on the library defaults. If the libraries'
  guesses differ from the current `mime_guess` output for any
  asset that ships in `web/dist/`, document the diff in the PR
  and adjust if it would cause a browser regression.
- Cache-Control:
  - `index.html` should remain `no-store` so a stale shell
    doesn't reference deleted hashed assets after deploy.
  - Hashed `assets/*` should remain `public, max-age=31536000, immutable`.
  - If `axum-embed`'s defaults don't match this, layer a small
    `set_response_header` middleware on the fallback service to
    override Cache-Control per path, or fork in a tiny adapter
    layer.
- Update the SPA-not-available fallback. Today
  `spa_not_available_response` returns an inline HTML 503 when
  the bundle is absent. Decide:
  - If `axum-embed` has zero entries, `Router::fallback_service`
    will surface a 404. Replace with an equivalent 503 + inline
    HTML via a small wrapping service, or accept a plain 404 and
    document the change.
- Remove `WebAssetSource`, `LoadedAsset`, `load`,
  `normalise_rel_path`, `is_inside`, `is_hashed_asset`,
  `content_type_for`, `cache_control_for`, `is_spa_request`,
  `serve_web_request`, `spa_not_available_response` from `web.rs`.
- Remove `mime_guess` from `crates/backend-backendd/Cargo.toml` if
  nothing else depends on it.
- The router in `server.rs` (or wherever 1710 placed it) wires
  the SPA via `Router::fallback_service(spa_service(...))`.
- Update existing `web/tests.rs` (if 1710 kept it) or
  `web.rs`'s submodule tests: replace asserts on the removed
  helper functions with end-to-end requests through the new
  service. Cover: GET `/` returns `index.html`, GET
  `/assets/<some-hashed-name>` returns the file with immutable
  Cache-Control, GET `/some/spa/route` falls back to `index.html`,
  GET `/../etc/passwd` returns 404.

## Out of scope

- Frontend changes. `web/` is unchanged.
- mTLS runtime API.
- Pre-compression of assets at build time (gzip/br). If we want
  to ship precompressed assets in a future ticket, both libraries
  support it; keep it out here.

## Acceptance criteria

- [ ] `crates/backend-backendd/src/web.rs` is ≤ ~50 lines.
- [ ] `rg -n 'fn normalise_rel_path|fn is_inside|fn cache_control_for|fn content_type_for|fn serve_web_request|fn spa_not_available_response' crates/backend-backendd/src/`
      returns nothing.
- [ ] `axum_embed` and `tower_http::services::{ServeDir, ServeFile}`
      are imported in `crates/backend-backendd/src/`.
- [ ] `mime_guess` is removed from `crates/backend-backendd/Cargo.toml`
      if no other code still uses it.
- [ ] GET `/` returns 200 with `Content-Type: text/html` and
      `Cache-Control: no-store` (or equivalent, e.g. `no-cache`)
      against the embedded build.
- [ ] GET `/assets/<one-of-the-built-hashed-files>` returns 200
      with `Cache-Control` indicating long-lived immutable
      caching.
- [ ] GET `/users` (a client-side route, no matching file) returns
      `index.html` content with a 200.
- [ ] GET `/../etc/passwd` returns 404.
- [ ] Dev mode: with `UNDERWRAP_BACKENDD_WEB_DIR` set to a
      directory containing a built `dist/`, the daemon serves
      from disk and applies the same SPA fallback.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` and
      `cargo clippy --all-targets --workspace -- -D warnings` pass.
- [ ] `cd web && npm test` and `cd web && npm run build` pass.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.
