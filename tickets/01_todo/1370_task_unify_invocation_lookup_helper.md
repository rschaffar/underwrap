# Task: Extract a shared open/finalize lookup helper in `runtime_api/invocations.rs`

Parent: `1300_story_backend_backendd_refinement.md`
Resolves: F-013 (review report `backend_backendd_2026-05-22.md`)

## Background

`handle_runtime_wrapper_invocation_open` (lines 130-183) and
`handle_runtime_wrapper_invocation_finalize` (lines 246-299) each
duplicate the same block:

1. `load_policy_document(store)?` (or 500-internal on error).
2. `resolve_invocation(store, principal, wrapper, resource, &policy)?`
   (or 500-internal on error).
3. Match all four variants of `InvocationLookup`:
   - `UnregisteredPair` → `Deny { reason: "unregistered-pair" }`.
   - `NoPermission` → `Deny { reason: "no-permission" }`.
   - `Denied(reason)` → `Deny { reason }`.
   - `Allowed { resource_id, permission }` → `(resource_id, permission.writable)`.

The two enums `RuntimeWrapperInvocationOpenResponse::Deny` and
`RuntimeWrapperInvocationFinalizeResponse::Deny` are distinct types
even though they share the same shape, so the deny-emission has to
be parameterised by the response constructor.

## Scope

- Extract a helper, e.g.:

  ```rust
  pub(crate) fn resolve_invocation_or_deny<F>(
      store: &dyn StorageBackend,
      principal: &str,
      wrapper_name: &str,
      resource_name: &str,
      into_deny: F,
  ) -> std::result::Result<(String, bool), Response<Full<Bytes>>>
  where
      F: Fn(String) -> Response<Full<Bytes>>,
  ```

  The helper loads the policy, calls `resolve_invocation`, and
  either returns `(resource_id, writable)` on `Allowed` or hands
  the reason string to `into_deny` and returns the response.
  Internal-error fallthroughs on policy load / lookup become a
  shared 500 inside the helper.

- Each handler collapses the 50-line block to:

  ```rust
  let (resource_id, writable) = match resolve_invocation_or_deny(
      store, principal, &body.wrapper_name, &body.resource_name,
      |reason| json_response(StatusCode::OK,
          &RuntimeWrapperInvocationOpenResponse::Deny { reason }),
  ) {
      Ok(t) => t,
      Err(response) => return response,
  };
  ```

  Mirror the same in `handle_runtime_wrapper_invocation_finalize`
  with the `FinalizeResponse::Deny` constructor.

## Out of scope

- Changing the `InvocationLookup` enum or `resolve_invocation`'s
  signature.
- Changing the reason strings on the wire.

## Acceptance criteria

- [ ] The four-variant match of `InvocationLookup` exists in
      exactly one place inside the helper.
- [ ] Both handlers' lookup-prelude collapses to one
      `resolve_invocation_or_deny(...)` call.
- [ ] `cargo test --workspace` passes (the existing tests in
      `runtime_api/invocations/tests.rs` should pass unchanged).
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
