# Task: Embedded-SPA smoke test in the backend integration suite

Parent: `290_story_admin_spa_post_rework_polish.md`

## Background

`crates/backend-backendd/src/web.rs` uses `rust-embed` with
`#[folder = "$UNDERWRAP_WEB_DIST"]`. If the env var points at an
empty directory (or the directory contains only `.gitkeep`), the
binary still compiles and starts, but serves an empty response at
`/`. Commit `37fd135` ("Restore web/dist/.gitkeep so rust-embed has
a non-empty default") was a band-aid: it makes the build succeed,
not the embed correct. No automated test catches "binary built fine
but the SPA isn't actually inside it."

## Scope

- Add an integration test (most likely in `crates/integration-tests/`
  or a new dedicated test in `crates/backend-backendd/tests/`) that:
  1. Starts a `BackendConfig` with an admin listener.
  2. Fetches `GET /` (or whatever route the SPA root is mounted on).
  3. Asserts the response status is 200 and the body contains a
     recognisable marker from `web/dist/index.html` (e.g. the
     `<script type="module"` Vite injects, or a known string from
     `<title>`).
- The test must run as part of `cargo test --workspace` so it gates
  the same builds that ship the binary.
- If the SPA is gated behind auth (login redirect), the assertion
  should still work on the login page HTML — pick a marker that's
  stable across deploys.

## Acceptance criteria

- [ ] A test fails the build when `$UNDERWRAP_WEB_DIST` points at an
      empty / placeholder-only directory.
- [ ] The same test passes against a real SPA bundle.
- [ ] The test does not require a full headless browser — a plain
      HTTP fetch is enough.

## Out of scope

- Asserting that the SPA *runs* (JavaScript executes correctly) —
  that's what Playwright is for. This test only proves the bytes
  arrived.
