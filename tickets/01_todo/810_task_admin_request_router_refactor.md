# Task: Refactor `handle_admin_request` into per-prefix sub-routers

Parent: `700_story_backend_modularization.md`

## Scope

After 760-800, every admin handler lives in its own `admin_api/<subdomain>.rs`
module, but `handle_admin_request` is still a single ~500-line flat
`match` that calls into each module. This ticket replaces that with a
short dispatcher that delegates URL routing to each subdomain's own
`route` function.

### Per-subdomain `route` function

Each `admin_api/<subdomain>.rs` gains a `pub(crate) fn route` of the
form:

```rust
pub(crate) async fn route(
    method: &Method,
    path: &str,
    state: &AdminRuntimeState,
    principal: &Principal,
    request: &mut Option<Request<Incoming>>,
) -> Option<Response<Full<Bytes>>>;
```

It returns `Some(response)` if the (method, path) pair matched a route
this subdomain owns (including path-parameterised routes like
`/v1/admin/stores/{id}/unlock`), and `None` otherwise. `request` is an
`Option` so the function can take ownership of the body when it
matches and leave it untouched when it doesn't.

Subdomains where every handler is GET-only (e.g. `overview`) can use a
simpler signature; pick whichever shape is cleanest per subdomain as
long as the dispatcher stays uniform.

### `crates/backend-backendd/src/admin_api/mod.rs`

`handle_admin_request` shrinks to roughly:

```rust
pub async fn handle_admin_request(req, state) -> Response {
    // 1. SPA fallback (delegate to crate::web)
    // 2. Origin check
    // 3. Public auth endpoints (auth::route_public, bootstrap::route_public)
    // 4. authenticate_admin + check_csrf
    // 5. Logout (auth-required)
    // 6. resolve_principal
    // 7. Try each sub-router in URL-prefix order:
    //    stores::route, users::route, client_instances::route,
    //    secrets::route, policy::route, resources::route,
    //    pairs::route, overview::route, auth::route (password-change).
    // 8. Fall through to 404.
    // 9. attach_csrf_cookie before returning.
}
```

Target: under ~80 lines.

### Path-parameterised routes

The current dispatcher has special-cased
`/v1/admin/stores/{id}/{rotate-key,unlock,lock}`,
`/v1/admin/users/{id}/password`, `/v1/admin/users/{id}`,
`/v1/admin/client-instances/{id}`, `/v1/admin/stores/{id}` blocks at
the top of `handle_admin_request`. Each of those moves into the
matching subdomain's `route()`.

## Acceptance criteria

- [ ] Each admin subdomain module exports a `route` (or `route_public`
      / `route` pair, for auth + bootstrap) function.
- [ ] `handle_admin_request` is under ~80 lines and contains no
      per-endpoint match arms; only the dispatch loop and the
      cross-cutting gates.
- [ ] No URL behaviour change: every (method, path) that responds
      today still produces a byte-identical response.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] Playwright E2E (`cd web && npm run test:e2e` inside
      `nix develop`) still passes.

## Out of scope

- Adopting a routing framework / macros. Plain functions only.
- Changing what URL surface each subdomain owns.
- Splitting `AdminRuntimeState`. After this ticket lands, decide
  whether it's worth the churn; that's a separate decision.
