# Task: Drop dead `#[allow(clippy::result_large_err)]` attributes

Parent: `1130_story_admin_api_refinement.md`
Resolves: F-008 (review report `admin_api_2026-05-22.md`)

## Background

`crates/backend-backendd/src/admin_api/` carries ~31 occurrences of
`#[allow(clippy::result_large_err)]`. The lint triggers only on
function signatures whose return type is `Result<_, E>` where `E` is
larger than the configured threshold. A function that returns
`Response<Full<Bytes>>` directly cannot be the lint's target — those
allows are leftovers from a point in time when the handlers returned
`Result<_, _>`.

Approximate breakdown from the review:

- `users.rs:78, 96, 118, 131, 193, 207, 251, 324`
- `pairs.rs:61, 90, 179`
- `policy.rs:174, 194, 264`
- `resources.rs:117, 144, 321, 462`
- `stores.rs:230, 292, 309, 499, 513, 599, 1064, 1167`
- `csrf.rs:127` (`resolve_principal`) and `auth.rs:239`
  (`handle_admin_password_change`) — these two **do** return `Result`
  in some shape and may be legitimate.

Most of the rest decorate handlers whose return type is
`Response<Full<Bytes>>`.

## Scope

Mechanical, with `cargo clippy` as the oracle:

1. Remove every `#[allow(clippy::result_large_err)]` in
   `crates/backend-backendd/src/admin_api/` whose decorated function's
   return type does **not** contain `Result<...>`.
2. Run `cargo clippy --all-targets --workspace -- -D warnings`.
3. If the lint fires on a function whose signature genuinely is
   `Result<...>` with a large Err, re-add the allow on that function
   only. Note in the commit message which ones survived (probably
   `resolve_principal` and possibly `handle_admin_password_change`'s
   inner helper).
4. After this ticket, roughly 25-30 of the 31 occurrences should be
   gone. Exact count depends on what the lint still flags.

Line numbers in the review are pre-1140; after the rename sweep some
will have shifted. Use the function-name oracle ("does its return type
include `Result`?") rather than the line-number list to decide each
case.

## Out of scope

- Reshaping any function's signature.
- Touching `#[allow(...)]` attributes other than
  `clippy::result_large_err`.
- Files outside `crates/backend-backendd/src/admin_api/`. (If the same
  smell exists in other crates, it gets its own sweep — not this
  ticket.)

## Acceptance criteria

- [ ] `rg -n 'clippy::result_large_err' crates/backend-backendd/src/admin_api/`
      returns at most a small handful of matches; the rest are gone.
- [ ] Every remaining allow decorates a function whose return type
      contains `Result`.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
