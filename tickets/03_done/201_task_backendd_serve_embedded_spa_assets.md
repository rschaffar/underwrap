# Task: Serve the SPA bundle from backendd

Parent: `200_story_backendd_serves_spa_and_admin_tls.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Add a static asset handler to the admin listener that serves the SPA `dist/` from one of:
  - **embedded mode**: assets compiled into the binary via `rust-embed`
  - **disk mode** (dev): assets read from a directory pointed at by `UNDERWRAP_BACKENDD_WEB_DIR`
- `GET /` and any non-`/v1/...` GET that does not match a file return `index.html` so client-side routing works.
- Set `Content-Type` per extension (`mime_guess`) and conservative cache headers:
  - `Cache-Control: public, max-age=31536000, immutable` for hashed asset files (e.g. `assets/index-<hash>.js`)
  - `Cache-Control: no-store` for `index.html`
- 404s from inside `/v1/...` continue to return JSON, not HTML.
- Asset routes are public (no auth) since the SPA bundle itself isn't a secret; auth is enforced on the `/v1/admin/...` API the SPA then calls.

## Key files likely involved

- `crates/backend-backendd/src/server.rs`
- New module `crates/backend-backendd/src/web.rs`
- `crates/backend-backendd/Cargo.toml` (add `rust-embed`, `mime_guess`)

## Acceptance criteria

- [ ] Embedded mode: `nix build .#default` produces a binary that serves the SPA without any external files.
- [ ] Dev disk mode: `UNDERWRAP_BACKENDD_WEB_DIR=web/dist cargo run -p backend-backendd ...` serves the locally built SPA.
- [ ] History fallback returns `index.html` for any non-API GET that doesn't match a file.
- [ ] API 404s (under `/v1/...`) return JSON.
- [ ] MIME types and cache headers are correct.
- [ ] Tests cover: history fallback, MIME for `.js`/`.css`/`.svg`/`.woff2`, cache headers for hashed vs unhashed assets, that endpoints under `/v1/admin/...` are not shadowed.
