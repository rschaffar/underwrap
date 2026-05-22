# Task: Unify body/handler split into the `*_endpoint` shape

Parent: `1130_story_admin_api_refinement.md`
Resolves: F-003, F-004 (review report `admin_api_2026-05-22.md`)

## Background

Three patterns coexist for "parse the JSON body, then call the body-taking
handler":

1. `handle_admin_X_endpoint(request)` parses, then calls
   `handle_admin_X(state, principal, &body)` — used in `policy.rs`,
   `pairs.rs`, `resources.rs`, and the `write`/`rename` halves of
   `secrets.rs`.
2. `handle_admin_X(request)` parses, then calls
   `do_admin_X(state, principal, body)` — used in `users.rs`,
   `client_instances.rs`, and most of `stores.rs`.
3. No split at all — `auth.rs::handle_admin_login`, `bootstrap.rs::handle_admin_bootstrap`,
   `secrets.rs::handle_admin_secret_{read,list,delete}` parse inline
   because no unit test needs the body-taking form.

User decision 2026-05-22: standardize on pattern 1. The body-taking
sibling drops the `_endpoint` suffix and is what unit tests reach for;
the `_endpoint` wrapper is `parse_json_body` + delegate.

Two of the existing pattern-1 wrappers (`handle_admin_secret_write_endpoint`,
`handle_admin_secret_rename_endpoint`) currently also call
`admin_store(state, &body.store)` purely as an existence check before
delegating — the body-taker then calls `admin_store()` a second time.
That double lookup goes away as part of this sweep.

## Scope

For every body-taking admin handler that is unit-tested, the shape after
this ticket is:

```rust
pub(crate) async fn handle_admin_X_endpoint(
    state: &AdminRuntimeState,
    principal: &Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<XRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    handle_admin_X(state, principal, &body)
}

pub(crate) fn handle_admin_X(
    state: &AdminRuntimeState,
    principal: &Principal,
    body: &XRequest,
) -> Response<Full<Bytes>> { ... }
```

Concrete renames (the `_endpoint` form is the request-taker; the
un-suffixed form is the body-taker that drops `do_admin_` and takes
`&body`):

- `users.rs::handle_admin_user_create` (request-taker) →
  `handle_admin_user_create_endpoint`; `do_admin_user_create` →
  `handle_admin_user_create`. Same for the update / delete pairs.
- `client_instances.rs::handle_admin_client_instance_create` →
  `..._endpoint`; `do_admin_client_instance_create` →
  `handle_admin_client_instance_create`. Same for update / delete.
- `stores.rs::handle_admin_store_create` → `..._endpoint`;
  `do_admin_store_create` → `handle_admin_store_create`. Same for the
  other `handle_admin_store_X` / `do_admin_store_X` pairs.

Some pattern-1 handlers in `policy.rs` already use the target shape
verbatim — those need no rename, but verify the body-taker borrows
(`&body`) rather than owning.

For the no-split handlers in `auth.rs`, `bootstrap.rs`, and the
read/list/delete halves of `secrets.rs`: leave them as single functions.
The rule is "split when a unit test wants the body-taking form", not
"split everything".

Fold the F-004 double-lookup fix into the same diff: in
`secrets.rs::handle_admin_secret_write_endpoint` and
`handle_admin_secret_rename_endpoint`, the wrapper after this ticket is
literally `parse_json_body` + delegate; the redundant
`admin_store(state, &body.store)` existence check disappears. The
body-taker's own `admin_store()` call stays — it still needs the handle.

Update every existing unit test that called a `do_admin_*` form to call
the renamed `handle_admin_*` instead.

Update `route()` dispatch tables that referenced the old names.

## Out of scope

- Touching any handler that isn't currently split (no new splits, no
  collapses of existing splits beyond the F-004 case).
- Any logic change inside the body-takers beyond the F-004 lookup
  removal. Pure mechanical rename + signature change to `&body`.
- The `policy.rs` write helper (that's 1160).
- `stores.rs` file split (that's 1200; this ticket only renames within
  the existing file).

## Acceptance criteria

- [ ] `rg -n '\bdo_admin_' crates/backend-backendd/src/admin_api/`
      returns no matches.
- [ ] Every `handle_admin_*_endpoint` body is `parse_json_body` +
      delegate; no other work happens in `_endpoint` wrappers.
- [ ] Body-takers take `&body: &XRequest`, not `body: XRequest`.
- [ ] `secrets.rs::handle_admin_secret_write_endpoint` and
      `handle_admin_secret_rename_endpoint` no longer call
      `admin_store` directly.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
