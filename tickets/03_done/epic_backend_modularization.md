# Epic: Modularize `backend-backendd` (split `server.rs`)

## Background

`crates/backend-backendd/src/server.rs` is 9 767 lines (~6 300 code +
~3 400 tests) and mixes ten unrelated concerns in one file:
configuration types, store-registry data model, mTLS plumbing, HTTP
response helpers, runtime-state struct, the three runtime endpoints,
~15 admin endpoint families, document load/save helpers, the session
+ CSRF gate, and a 560-line URL dispatcher. The sibling files in the
crate (`auth.rs`, `users.rs`, `web.rs`, `runtime_clients.rs`) are
already cleanly scoped — only `server.rs` is the problem.

The result is that almost every change to the backend touches the same
giant file, and finding the right place to make a change requires
scrolling past several unrelated subsystems. The "single reason to
touch" property is gone.

This epic is **pure modularization**. No behavioural change, no public
API change, no security change. Every step is mechanical and verified
by `cargo test --workspace` staying green.

## Goals

1. `server.rs` is gone. Its responsibilities live in modules each
   named after the concern they own.
2. Every new module has a single reason to be touched: a config
   change, a TLS change, a runtime-endpoint change, an admin-endpoint
   change for a specific URL prefix, etc. The largest remaining file
   is on the order of 1 000 - 1 200 lines (the stores admin
   subdomain); everything else is in the 100 - 500 range.
3. `handle_admin_request` is a short dispatcher that delegates to one
   sub-router per URL prefix instead of a 560-line flat `match`.
4. Unit tests live next to the code they exercise.
5. `cargo test --workspace`, `cargo fmt --check`, and
   `cargo clippy --all-targets --workspace -- -D warnings` stay green
   on every intermediate commit. The VM test still passes at the end
   of the epic.

## Target layout

```
crates/backend-backendd/src/
├── lib.rs                          # re-exports public surface
├── main.rs                         # unchanged
├── auth.rs                         # unchanged
├── users.rs                        # unchanged
├── web.rs                          # unchanged
├── runtime_clients.rs              # unchanged
│
├── config.rs                       # HttpsTlsConfig, HttpsConfig,
│                                   #   AdminConfig, BackendConfig
├── store_registry.rs               # StoreState, RegistryLookupError,
│                                   #   LoadedStoreRecord, StoreRegistry,
│                                   #   load/save_store_records,
│                                   #   open_initial_stores,
│                                   #   is_valid_store_id, generate_store_id
├── tls.rs                          # build_https_acceptor,
│                                   #   FingerprintClientCertVerifier,
│                                   #   load_cert_chain, load_private_key
├── runtime_state.rs                # AdminRuntimeState
│
├── http/
│   ├── mod.rs                      # run(), https_accept_loop,
│   │                               #   handle_https_connection,
│   │                               #   handle_https_request (admin/service split)
│   ├── responses.rs                # json_response, error_response,
│   │                               #   store_locked_response, parse_json_body
│   └── origin.rs                   # validate_admin_config (origin bit),
│                                   #   check_admin_origin, referer_to_origin
│
├── storage/
│   ├── mod.rs                      # re-exports
│   ├── documents.rs                # load/save_{policy,resources,pairs}_document,
│   │                               #   resource_secret_dependents
│   └── secret_entries.rs           # read/write/delete/list_secret_entries,
│                                   #   resource_blob_entries,
│                                   #   validate_and_compose_secret_id
│
├── runtime_api/
│   ├── mod.rs                      # handle_service_request (dispatcher)
│   ├── principal.rs                # RuntimeRequestPrincipal,
│   │                               #   resolve_runtime_client,
│   │                               #   validate_runtime_client_mappings
│   ├── wrappers.rs                 # handle_runtime_wrapper_register
│   └── invocations.rs              # handle_runtime_wrapper_invocation_{open,finalize},
│                                   #   InvocationLookup, resolve_invocation,
│                                   #   bump_pair_last_used
│
└── admin_api/
    ├── mod.rs                      # handle_admin_request (~50 lines now)
    ├── audit.rs                    # log_admin_event + audit emitters
    ├── csrf.rs                     # AdminAuth, AdminAuthError,
    │                               #   authenticate_admin, check_csrf,
    │                               #   resolve_principal, attach_csrf_cookie
    ├── auth.rs                     # handle_admin_login / logout / whoami /
    │                               #   password_change
    ├── bootstrap.rs                # bootstrap status + bootstrap +
    │                               #   audit_bootstrap_attempt
    ├── users.rs                    # admin user CRUD
    ├── stores.rs                   # store CRUD + unlock/lock/rotate-key/reencrypt
    ├── client_instances.rs         # client-instance CRUD
    ├── secrets.rs                  # /v1/admin/secret/*
    ├── policy.rs                   # /v1/admin/policy/* + wrapper-permission CRUD
    ├── resources.rs                # resource CRUD + blob get/put
    ├── pairs.rs                    # pair CRUD
    └── overview.rs                 # /v1/admin/ui/overview
```

## Product decisions

- **No behavioural change.** Every diff in this epic is a move + import
  fix-up. If a function is moved, it is not also renamed or refactored.
- **No public API change.** `lib.rs` re-exports keep `BackendConfig`,
  `run`, etc. visible at the same paths so `main.rs` and the
  integration tests don't have to change.
- **`AdminRuntimeState` stays as one struct.** Splitting it up forces
  every handler signature to change, which is a separate decision best
  made *after* the file split, when the natural cuts are visible. A
  follow-up ticket can revisit it.
- **Sub-routers, not a framework.** Each `admin_api/<subdomain>.rs`
  exposes one `route(method, path, state, principal, request) ->
  Option<Response>` function. `handle_admin_request` tries each in URL
  prefix order. No new dependencies, no macros.
- **One PR per task.** Every task ticket is intended to be one
  mechanical PR that leaves the tree green. Tasks are ordered so that
  later tasks build on earlier ones (e.g. the storage helpers move
  before the admin subdomains that depend on them).
- **Tests move last.** The first batch of tasks keeps the `#[cfg(test)]`
  block in `server.rs` (re-exported via `pub(crate) use` where
  needed). Once all production code has moved, one final task
  relocates the test functions to live next to their target modules.

## Stories

- `700_story_backend_modularization.md` — single rollout story
  enumerating the task tickets below.

## Out of scope

- Renaming functions, changing their signatures, or replacing
  `anyhow::Result` with custom error types. Pure moves only.
- Splitting `AdminRuntimeState`. Tracked as an explicit deferred
  decision, not a planned task.
- Changing the URL surface or wire types. The SPA, daemon, and
  integration tests must keep working through every step.
- Touching `auth.rs`, `users.rs`, `web.rs`, or `runtime_clients.rs`.
  Those are already cleanly scoped.
- Documentation updates (`docs/backend.md` etc.) beyond mentioning
  the new module names where they appear. The architecture itself
  isn't changing.
