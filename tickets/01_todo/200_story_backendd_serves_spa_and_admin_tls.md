# Story: backend-backendd serves the SPA directly over TLS

Parent: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Background

Today the browser talks to the Node.js Next.js app; the Node app forwards to `backend-backendd` over plain HTTP on `127.0.0.1`. The new design eliminates the Node tier: `backend-backendd` serves both the admin JSON API and the SPA static assets, with TLS on the admin listener.

## Desired design

- Admin listener gains rustls TLS support, mirroring the service listener's setup (`build_tls_acceptor` in `crates/backend-backendd/src/server.rs`).
- New static-asset routes serve the SPA bundle embedded via `rust-embed`. History fallback returns `index.html` for unknown non-`/v1/*` paths so the SPA router can take over.
- The Nix package produces a single binary that bundles the SPA `dist/`. No `nodejs` in the runtime closure.
- Cargo build also works in dev with the SPA pointed at `web/dist` on disk via an env var, so developers don't need to rebuild the Rust binary for every UI change.

## Tasks

1. `200_task_backendd_admin_tls_and_origin_hardening.md`
2. `201_task_backendd_serve_embedded_spa_assets.md`
3. `202_task_nix_spa_build_and_single_binary_packaging.md`

## Acceptance criteria

- [ ] Admin listener can terminate TLS (cert/key files configurable).
- [ ] SPA assets are served by the backend (embedded in release, filesystem-pointed in dev).
- [ ] All non-API GET requests fall back to `index.html` so client-side routing works.
- [ ] `nix build .#default` yields a single binary that runs both the API and the UI with no Node.js runtime dependency.
- [ ] Tests verify TLS handshake, history fallback, MIME types, and asset cache headers.
