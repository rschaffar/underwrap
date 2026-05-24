# Task: Unify runtime_api response shape on `axum::Response`

Resolves: B-RT-1, B-RT-2, B-RT-8.

## Problem

The backend has two response dialects inside one HTTP listener:

- Admin API returns `axum::response::Response` via
  `AdminError::into_response()`.
- Runtime API returns `hyper::Response<Full<Bytes>>` via hand-built
  builders in `crates/backend-backendd/src/runtime_api/responses.rs`
  (`json_response`, `error_response`, `store_locked_response`).

The runtime dialect forces ~30 lines of boilerplate per handler — six
near-identical blocks in `wrappers.rs`, more in `invocations.rs`:

```rust
let mut x = match load_X_document(store) {
    Ok(d) => d,
    Err(e) => return error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal",
        &format!("loading X: {e:#}"),
    ),
};
```

Sites: `crates/backend-backendd/src/runtime_api/wrappers.rs:68-87,154-172,228-264`;
`runtime_api/invocations.rs:101-119,192-211,312-353`.

Also: `runtime_api/mod.rs:65-68` logs the same error string twice
(`err = format!("{e:#}")` field + `"...{e:#}"` message interpolation) —
the structured JSON output carries the string twice.

## Security boundary

The wire envelope must stay byte-identical (the daemon's
`BackendClient` parses by shape). Quotes from `CLAUDE.md` and
`docs/security.md` to honour:

- "No fallback modes — if a security mechanism fails, the wrapper
  aborts."
- The runtime API's `Allow` / `Deny { reason }` JSON shape and
  status codes are part of the security contract.

## What to do

1. Build a `RuntimeError` type with an `IntoResponse` impl mirroring
   `AdminError`'s envelope shape exactly. Variants for: internal,
   bad-request, store-locked, not-found, etc. (mirror what
   `runtime_api/responses.rs` currently produces).
2. Add an `IntoInternal`-style adapter so `result.map_err(|e|
   e.into_internal("loading X document"))?` reads cleanly.
3. Convert runtime handlers to return `Result<impl IntoResponse,
   RuntimeError>` and use `?`. The 30-line `match` blocks collapse
   to one-liners.
4. Delete `crates/backend-backendd/src/runtime_api/responses.rs`
   when nothing imports it.
5. Fix the double-logging in `runtime_api/mod.rs:65-68` — keep the
   structured `err` field, drop the interpolation from the message
   template.

## Acceptance

- `cargo test --workspace`
- `cargo test -p integration-tests`
- `nix build .#checks.x86_64-linux.vm-test`
- Existing JSON-shape snapshots in `runtime_api/*/tests.rs` stay green
  byte-for-byte.
- `cargo clippy --all-targets --workspace -- -D warnings`
