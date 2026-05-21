# Task: Relocate unit tests to live next to the code they exercise

Parent: `700_story_backend_modularization.md`

## Scope

After 710-810, `server.rs` consists of (roughly) the `pub fn run`
bootstrap + `handle_https_request` + the two `#[cfg(test)]` modules at
the bottom — `store_registry_tests` (6231-6322) and the giant `tests`
module (6324-9767, ~3 400 lines, 102 test functions).

This ticket moves each test next to the code it tests. The result:
`server.rs` is replaced with a much smaller `http/mod.rs` (or
similar) that holds only the bootstrap + accept loop + admin/service
split.

### Mechanical rule

Every test goes into the `#[cfg(test)] mod tests` of the module whose
production function it covers. Use the function names that appear in
the test (most tests are named `<function_under_test>_<behaviour>`).

Examples of the mapping:

- `authenticate_admin_*`, `check_admin_origin_*`, `referer_to_*`,
  `attach_csrf_cookie_*` → `admin_api/csrf.rs` (or
  `http/origin.rs` for `check_admin_origin_*`).
- `resolve_runtime_client_*` → `runtime_api/principal.rs`.
- `admin_store_*` → wherever `admin_store` ended up (see 800).
- `resolve_principal_*` → `admin_api/csrf.rs`.
- `bootstrap_*` → `admin_api/bootstrap.rs`.
- `validate_admin_config_*` → `http/origin.rs`.
- The `store_registry_tests` module moves wholesale to
  `store_registry.rs`.

### Test helpers

Many tests share helpers (`test_admin_state`, `seeded_admin_state`,
`session_headers`, `service_state_with_runtime_client`, …). Move
these into one of:

- `crates/backend-backendd/src/test_support.rs` behind
  `#[cfg(test)]`, if they're used from multiple modules.
- The single module that uses them, if they aren't.

Decide per helper. The criterion is: a helper lives in
`test_support.rs` iff it's referenced by tests in more than one
module.

### Finalisation

Once every test has moved, delete `server.rs` (if anything remains
in it that wasn't claimed by an earlier task, treat that as a
bug in the earlier task and fix it in this PR). Move
`pub fn run`, `https_accept_loop`, `handle_https_connection`, and
`handle_https_request` into `http/mod.rs` if they're not already.

### `lib.rs`

Update to declare modules at top level (`mod http;`, `mod admin_api;`,
etc.) without `mod server;`. Re-exports stay byte-identical.

## Acceptance criteria

- [ ] `crates/backend-backendd/src/server.rs` no longer exists.
- [ ] Every test function from the old `server.rs` lives in a
      `#[cfg(test)] mod tests` block in the module whose production
      function it exercises (or in `test_support.rs` for shared
      helpers).
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
