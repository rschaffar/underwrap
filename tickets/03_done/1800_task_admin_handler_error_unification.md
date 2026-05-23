# Task: Unify admin handler error flow via `AdminError: IntoResponse`

## Background

The admin API in `crates/backend-backendd/src/admin_api/` is ~6800
lines, and a large fraction of that is repeated error-plumbing
boilerplate. Handlers return `Response<Full<Bytes>>` directly and
hand-write every failure branch:

```rust
let store = match admin_store(state, &body.store) {
    Ok(s) => s,
    Err(resp) => return resp,
};
let doc = match load_resources_document(store.as_ref()) {
    Ok(d) => d,
    Err(e) => return error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal",
        &format!("loading resources document: {e:#}"),
    ),
};
```

There are ~140 `return error_response(...)` sites and ~20 `Err(resp)
=> return resp` matches across the admin tree. Each handler is also
split into a `handle_admin_X(state, body)` free function plus a thin
`axum_X` wrapper, doubling the per-endpoint surface for no
non-test caller.

The wire contract — `ServiceErrorResponse { code, message }` JSON
envelope, status codes, audit-emit order — is fine and must not
change. Only the **call-site shape** needs to.

## Scope

One PR that:

1. Introduces a crate-private `AdminError` enum in
   `crates/backend-backendd/src/admin_api/error.rs` covering the
   failure modes that handlers currently encode by hand:

   - `StoreNotFound(String)`           → 404 `not_found`
   - `StoreLocked(String)`             → 503 `store_locked`
   - `NotFound(&'static str)`          → 404 `not_found`
   - `Conflict { code, message }`      → 409
   - `BadRequest { code, message }`    → 400
   - `Unauthorized` / `Forbidden`      → 401 / 403 (if used)
   - `Internal(anyhow::Error, &'static str)` → 500 `internal`,
      message prefixed with the static context (`"loading
      resources document"` etc.) so log/audit lines stay readable

   Plus the conversions handlers need today:

   - `From<anyhow::Error>` is **not** implemented — handlers must
     pick the context string explicitly, mirroring the current
     `&format!("loading X document: {e:#}")` discipline.
   - `impl IntoResponse for AdminError` emits a
     `ServiceErrorResponse` body with the same JSON shape
     `error_response` / `store_locked_response` produce today.

2. Migrates **every** admin handler from
   `-> Response<Full<Bytes>>` to
   `-> Result<Json<T>, AdminError>` (or `Result<(StatusCode,
   Json<T>), AdminError>` where the success status is not 200).
   Modules in scope:

   - `auth.rs`, `bootstrap.rs`, `csrf.rs` (where it emits
     responses)
   - `client_instances.rs`, `overview.rs`, `pairs.rs`,
     `policy.rs`, `resources.rs`, `secrets.rs`, `users.rs`
   - `stores/{mod,crud,lifecycle}.rs`

3. Folds each `handle_admin_X` free function into its `axum_X`
   handler. The split exists today only so tests can call the
   business logic without going through axum; replace those test
   call sites with `Router::oneshot` (or with a small
   `call_handler` helper in `test_support.rs` that builds a
   `Request` and runs it through the assembled router). The new
   handler stays directly testable because it is plain
   `async fn(&State, Body) -> Result<..., AdminError>`.

4. Updates `admin_store` (currently in `stores/mod.rs`) and the
   `load_*_document` helpers in
   `crates/backend-backendd/src/storage/documents.rs` so they
   return `Result<_, AdminError>` directly. This is what makes the
   per-handler boilerplate collapse to `?`. Helpers used by both
   admin **and** runtime APIs must keep their existing
   `anyhow::Result<_>` return type — wrap at the admin call site
   via `.map_err(|e| AdminError::Internal(e, "loading X
   document"))` (or a tiny extension trait) rather than tying
   `runtime_api/` to `AdminError`.

5. Deletes `http/responses.rs::{json_response, error_response,
   store_locked_response}` once no caller remains. `Body<T>` stays
   (its rejection should be updated to return
   `AdminError::BadRequest { code: "invalid_request", message }`
   so the envelope is unchanged).

## Out of scope

- Any change to the **wire contract**: status codes, the
  `ServiceErrorResponse` envelope, the `code` strings, the
  audit-event order/shape, the OpenAPI document.
- Any change to the **runtime API** (`runtime_api/`). It keeps
  returning `Response<Full<Bytes>>` for now — it has its own
  much shorter set of failure shapes and the mTLS path is small
  enough that the boilerplate is not worth chasing in the same
  PR.
- Introducing an `AdminStore` request extractor or collapsing the
  per-domain audit helpers (items 4 and 5 from the review). Both
  are clean follow-ups once `AdminError` is in place; track
  separately if we want them.
- Touching `utoipa::path` annotations beyond what the new return
  type forces (the `responses(...)` blocks stay verbatim — the
  generated spec must be byte-identical).

## Acceptance criteria

- [ ] `crates/backend-backendd/src/admin_api/error.rs` exists and
      defines `AdminError` + `IntoResponse`.
- [ ] No file under `crates/backend-backendd/src/admin_api/`
      contains the literal substring `Err(resp) => return resp`.
- [ ] `grep -r 'return error_response' crates/backend-backendd/src/admin_api/`
      returns zero hits.
- [ ] `http/responses.rs` no longer exports `json_response`,
      `error_response`, or `store_locked_response`. (`Body<T>`
      and `MAX_REQUEST_BODY_BYTES` stay.)
- [ ] No `handle_admin_*` free function remains in
      `admin_api/`; tests reach handlers via the router (or via
      a single shared `call_handler` helper in `test_support.rs`).
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` and
      `cargo clippy --all-targets --workspace -- -D warnings`
      pass.
- [ ] `cd web && npm test` passes (the FE consumes the JSON
      envelope; this catches accidental wire-shape drift).
- [ ] The generated OpenAPI spec is **byte-identical** to the
      pre-PR spec. The drift test in
      `crates/backend-backendd/tests/openapi_export.rs` already
      enforces this; make sure it stays green without snapshot
      regeneration.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.

## Notes for the implementer

- This is a no-behavior-change refactor. Land it as one PR rather
  than module-by-module — a half-migrated tree is the exact
  "parallel-with-legacy" intermediate state we don't want.
- Suggested commit sequence inside the PR:
  1. Add `AdminError` + `IntoResponse` + the helper conversions
     on `admin_store` / `load_*_document` (no handler changes
     yet; should compile and tests still pass against the old
     return shapes via temporary adapters).
  2. Migrate handlers module by module, dropping the
     `handle_admin_X` free functions and updating their tests as
     each module lands.
  3. Delete `json_response` / `error_response` /
     `store_locked_response` and remove the temporary adapters.
- When a `match` was previously discriminating between several
  `UserMutationError` variants to pick different audit reasons
  (`users.rs::user_mutation_response`), keep the discriminator —
  it should now produce the `AdminError` variant *and* the audit
  reason string in one place.
