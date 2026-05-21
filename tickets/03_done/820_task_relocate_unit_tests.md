# Task: Relocate unit tests to live next to the code they exercise

Parent: `700_story_backend_modularization.md`

## Scope

After 710-810, `server.rs` consists of (roughly) the `pub fn run`
bootstrap + `handle_https_request` + the giant `#[cfg(test)] mod
tests` block at the bottom (~3 400 lines, 102 test functions) plus a
small `store_registry_tests` block.

This ticket moves each test next to the code it tests. The result:
`server.rs` is replaced with a much smaller `http/mod.rs` (or
similar) that holds only the bootstrap + accept loop + admin/service
split.

### Layout: sibling `tests.rs` per module, not inline `mod tests {}`

Rust supports inline unit tests (`#[cfg(test)] mod tests { ... }`
at the bottom of every production file). It is idiomatic, but for a
crate this size the bottom-of-file inline blocks balloon the
production files with hundreds of lines of unrelated test scaffolding
— exactly the kind of mixed-concerns problem this epic is trying to
get rid of.

The right shape for this codebase is **same-module privacy, separate
file**:

```
src/admin_api/csrf.rs           # production
src/admin_api/csrf/tests.rs     # tests for csrf.rs
```

with `csrf.rs` ending in:

```rust
#[cfg(test)]
mod tests;
```

That declaration makes `csrf/tests.rs` a *submodule* of `csrf`, so
`use super::*;` inside it reaches every private and `pub(crate)` item
in `csrf.rs` — same privacy as an inline `mod tests { }` block.
Production files stay free of test code; tests still live next to
the code they exercise and can poke at internals.

This is the pattern used by `tokio`, `serde`, `hyper`, and most
production Rust codebases. It's also closer to the Java mental model
of "test sibling to impl" without sacrificing Rust's module-private
access.

For modules that have very few tests (one or two short functions),
inline `mod tests { ... }` at the bottom of the production file is
fine — don't force a separate file just to hold five lines. The
threshold is judgement: if the tests are longer than the production
code, split them out.

### Per-module mapping

Apply the sibling-file pattern to every module that ends up holding
non-trivial tests:

```
src/admin_api/csrf.rs           + src/admin_api/csrf/tests.rs
src/admin_api/auth.rs           + src/admin_api/auth/tests.rs
src/admin_api/bootstrap.rs      + src/admin_api/bootstrap/tests.rs
src/admin_api/users.rs          + src/admin_api/users/tests.rs
src/admin_api/stores.rs         + src/admin_api/stores/tests.rs
src/admin_api/client_instances.rs + src/admin_api/client_instances/tests.rs
src/admin_api/resources.rs      + src/admin_api/resources/tests.rs
src/admin_api/pairs.rs          + src/admin_api/pairs/tests.rs
src/admin_api/policy.rs         + src/admin_api/policy/tests.rs
src/admin_api/secrets.rs        + src/admin_api/secrets/tests.rs
src/admin_api/overview.rs       + src/admin_api/overview/tests.rs (if any)
src/runtime_api/principal.rs    + src/runtime_api/principal/tests.rs
src/runtime_api/wrappers.rs     + src/runtime_api/wrappers/tests.rs
src/runtime_api/invocations.rs  + src/runtime_api/invocations/tests.rs
src/http/origin.rs              + src/http/origin/tests.rs
src/http/responses.rs           + (inline if trivial)
src/store_registry.rs           + src/store_registry/tests.rs
src/storage/documents.rs        + src/storage/documents/tests.rs
src/storage/secret_entries.rs   + (inline if trivial)
src/config.rs                   + (inline if trivial)
src/tls.rs                      + (inline if trivial)
```

Mapping per test function family (most tests are named
`<function_under_test>_<behaviour>`):

- `authenticate_admin_*`, `check_csrf_*`, `attach_csrf_cookie_*`,
  `resolve_principal_*` → `admin_api/csrf/tests.rs`.
- `check_admin_origin_*`, `referer_to_origin_*`,
  `validate_admin_config_*` → `http/origin/tests.rs`.
- `handle_admin_login_*`, `handle_admin_logout_*`,
  `handle_admin_whoami_*`, `handle_admin_password_change_*` →
  `admin_api/auth/tests.rs`.
- `handle_admin_bootstrap_*`, `bootstrap_*` →
  `admin_api/bootstrap/tests.rs`.
- `handle_admin_user_*`, `do_admin_user_*`, `mutate_collection_*` →
  `admin_api/users/tests.rs`.
- `handle_admin_store_*`, `do_admin_store_*`, `admin_store_*`,
  `resolve_store_dir_*`, `rotation_lock_*`, `reencrypt_into_*` →
  `admin_api/stores/tests.rs`.
- `handle_admin_client_instance_*`, `do_admin_client_instance_*` →
  `admin_api/client_instances/tests.rs`.
- `handle_admin_resource_*` → `admin_api/resources/tests.rs`.
- `handle_admin_pair_*` → `admin_api/pairs/tests.rs`.
- `handle_admin_wrapper_permission*` → `admin_api/policy/tests.rs`.
- `handle_admin_secret_*` → `admin_api/secrets/tests.rs`.
- `resolve_runtime_client_*`, `get_policy_document_*`,
  `put_policy_document_*` → `runtime_api/principal/tests.rs`.
- `handle_runtime_wrapper_register_*` →
  `runtime_api/wrappers/tests.rs`.
- `handle_runtime_wrapper_invocation_*`, `bump_pair_last_used_*` →
  `runtime_api/invocations/tests.rs`.
- The current `store_registry_tests` block → `store_registry/tests.rs`.

### Test helpers

Many tests share helpers (`test_admin_state`, `seeded_admin_state`,
`session_headers`, `service_state_with_runtime_client`,
`TestStore`, `response_body_json`, `headers_with_session_cookie`,
`crud_state`, `admin_principal`, `RuntimeApiFixture`, …). The
test-only helpers are the trickiest part of this move because they
need to be reachable from multiple `<module>/tests.rs` files.

Put them in `src/test_support.rs` behind a crate-root cfg(test)
module:

```rust
// src/lib.rs
#[cfg(test)]
pub(crate) mod test_support;
```

Each `<module>/tests.rs` then does:

```rust
use crate::test_support::{test_admin_state, response_body_json, ...};
```

The criterion is straightforward: a helper lives in `test_support`
iff it's referenced by tests in more than one module. Helpers used
only inside one `tests.rs` stay there.

When a helper constructs a domain type that has only `pub(crate)`
fields, `test_support` may need to reach inside that module. Two
options:

1. Make the helper a method on the type (e.g.
   `impl AdminRuntimeState { #[cfg(test)] pub(crate) fn for_tests(...) }`).
2. Lift the helper into the module that owns the type, and only put
   *truly* cross-module helpers in `test_support`.

Prefer option 1 when the helper is a pure constructor; option 2 when
the helper composes multiple unrelated bits of state.

### Cfg(test) re-exports cleanup

During 710-810 we accumulated a number of `#[cfg(test)] pub(crate)
use ...` re-exports in `server.rs` so the inline test module could
reach moved functions via `super::*`. After this ticket, every test
lives next to its production module and reaches private items via
`use super::*;` directly — the `#[cfg(test)]` re-exports in
`server.rs` go away. Removing them is one of the acceptance
criteria.

### Finalisation

Once every test has moved, delete `server.rs` (if anything remains
in it that wasn't claimed by an earlier task, treat that as a
bug in the earlier task and fix it in this PR). Move
`pub fn run`, `https_accept_loop`, `handle_https_connection`, and
`handle_https_request` into `http/mod.rs`; `handle_admin_request`
into `admin_api/mod.rs` (if 810 didn't already do this).

The shared re-exports of `BackendConfig`, `HttpsConfig`,
`HttpsTlsConfig`, `AdminConfig`, `DEFAULT_STORES_FILE`,
`LoadedStoreRecord`, `RegistryLookupError`, `StoreRegistry`,
`StoreState`, `is_valid_store_id`, `load_store_records`,
`LoginRequest/Response`, `WhoamiResponse`, `BootstrapRequest`,
`BootstrapResponse`, `BootstrapStatusResponse`, and `run` all need to
end up reachable at `underwrap_backendd::server::*` and
`underwrap_backendd::*` exactly as they are today, so external
consumers (`integration-tests/`, `tests/ts_export.rs`, `main.rs`)
don't change.

Two reasonable shapes:

1. Drop `mod server;` entirely; export the same symbols at
   `underwrap_backendd::*` via `lib.rs` re-exports; update the four
   external consumer files to import from the crate root instead of
   `::server::`. Cleanest end state.
2. Keep `pub mod server;` as a *pure re-export shim* (one screen of
   `pub use ...;` lines, no code), so external consumers don't need
   to change at all. Slightly less clean but a zero-blast-radius move.

Pick the one that gives the smaller diff at the call sites. If it's
close, prefer option 1.

### `lib.rs`

Update to declare modules at top level (`mod http;`, `mod admin_api;`,
etc.) with `pub use ...` re-exports for whatever path the external
consumers ended up needing.

## Acceptance criteria

- [ ] `crates/backend-backendd/src/server.rs` either no longer
      exists, or is reduced to a `pub use ...` re-export shim with
      no executable code.
- [ ] Every test function from the old `server.rs` lives in a
      sibling `<module>/tests.rs` (or, for tiny modules, an inline
      `#[cfg(test)] mod tests { }` at the bottom of the production
      file). No `tests.rs` file is over ~600 lines.
- [ ] Test helpers used by more than one `tests.rs` live in
      `src/test_support.rs` behind `#[cfg(test)]`. Helpers used by
      exactly one `tests.rs` live in that `tests.rs`.
- [ ] All `#[cfg(test)] pub(crate) use ...` re-exports added to
      `server.rs` during 710-810 are gone — tests now import via
      `use super::*;` from the sibling `tests.rs`, or via
      `use crate::test_support::*;` for shared helpers.
- [ ] `cargo test --workspace` passes with the same number of test
      functions as before (102 + the surrounding workspace tests).
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.
- [ ] No production file in `crates/backend-backendd/src/` exceeds
      ~1 200 lines.

## Out of scope

- Adding new tests, rewriting tests, or merging duplicate tests.
- Changing the public surface of any module.
- Touching tests in `integration-tests/`.
- Moving anything from `integration-tests/` into `tests/`-dir
  black-box tests at the crate root — that's a separate decision
  about test taxonomy, not part of the modularization epic.
