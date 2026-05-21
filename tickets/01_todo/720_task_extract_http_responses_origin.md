# Task: Extract `http/responses` and `http/origin` modules

Parent: `700_story_backend_modularization.md`

## Scope

Pull the cross-cutting HTTP-layer helpers out of `server.rs` into a
small `http/` module. Every handler in the codebase calls
`json_response`, `error_response`, or `parse_json_body`; centralising
them is the prerequisite for splitting the handlers themselves.

### `crates/backend-backendd/src/http/mod.rs` (new)

Declares `responses` and `origin` submodules. Re-exports the helpers
at `crate::http::*` so call sites can `use crate::http::{json_response,
error_response, store_locked_response, parse_json_body};` etc.

### `crates/backend-backendd/src/http/responses.rs` (new)

Move verbatim from `server.rs`:

- `parse_json_body` (1028-1048)
- `json_response` (1050-1057)
- `error_response` (1059-1067)
- `store_locked_response` (1073-1081)

All `pub(crate)`.

### `crates/backend-backendd/src/http/origin.rs` (new)

Move verbatim from `server.rs`:

- `validate_admin_config` (684-696) — note this also validates the
  session-key-file path. Keep it intact; the function's job is
  "validate the admin config block", origin is one piece of that.
- `check_admin_origin` (698-731)
- `referer_to_origin` (733-747)

All `pub(crate)`.

### Call-site updates

`server.rs` now imports these from `crate::http::*`. No semantic
changes to any caller.

## Acceptance criteria

- [ ] `http/mod.rs`, `http/responses.rs`, `http/origin.rs` exist with
      the listed contents.
- [ ] `server.rs` no longer defines any of the listed functions.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.

## Out of scope

- Moving `https_accept_loop`, `handle_https_connection`, or
  `handle_https_request` into `http/`. Those move in a later task
  alongside `pub fn run`, once the runtime / admin API modules exist
  to be dispatched into.
- Refactoring `error_response` to take a structured error type. Pure
  move only.
