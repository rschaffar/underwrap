# Task: Final simplification sweep over the refined files

Parent: `1000_story_backend_backendd_refinement.md`
Resolves: trailing simplification pass for the refinement epic.

## Background

The earlier tickets in this epic each made one mechanical change.
Once the dust settles, a re-read against the simplification
checklist often catches remaining low-hanging fruit that wasn't
visible until the move.

The skill that produced the review report explicitly calls for this
pass when a refinement epic touches a lot of files.

## Scope

Re-read every file touched by tickets 1010 through 1110 — at
minimum:

- `crates/backend-backendd/src/server.rs`
- `crates/backend-backendd/src/lib.rs` (if 1110 Option 1)
- `crates/backend-backendd/src/auth.rs`
- `crates/backend-backendd/src/users.rs`
- `crates/backend-backendd/src/runtime_clients.rs`
- `crates/backend-backendd/src/web.rs`
- `crates/backend-backendd/src/test_support.rs`
- `crates/backend-backendd/src/store_registry.rs`
- `crates/backend-backendd/src/admin_api/{auth,bootstrap,client_instances,policy,pairs,resources,secrets,stores,users,overview}.rs`
- `crates/backend-backendd/src/http/mod.rs` and `origin.rs`
- `crates/backend-backendd/src/storage/{documents.rs,secret_entries.rs}`
- `crates/backend-backendd/src/runtime_api/{mod.rs,invocations.rs,wrappers.rs,principal.rs}`

For each, apply the category-7a checklist from the refinement
skill:

- Remove needless abstractions or single-caller wrapper types.
- Remove defensive branches that can't fire (trust internal code;
  validate only at system boundaries).
- Remove redundant clones, allocations, or `to_owned()` calls.
- Remove layers of indirection that exist for hypothetical future
  requirements rather than current callers.
- Replace hand-rolled logic with a short stdlib / iterator-combinator
  call where it reads better.
- Remove options, configs, or feature flags that are never set to
  anything but the default.
- Remove error-handling code paths for impossible states.
- Look for newly-visible cross-module duplication that the earlier
  tickets exposed but didn't act on.

Anything that flips observable behaviour is **out of scope for this
ticket** — file a separate ticket and surface it. This is the
"same behaviour, less code" sweep.

## Reporting

End the PR description with a short list of what was simplified and
what was deliberately left alone. The list is the audit trail for
the epic.

## Out of scope

- Renaming public items or wire types.
- Restructuring tests (the test layout was decided in 1070/1080/1090).
- New abstractions. This is a pruning pass, not a design pass.

## Acceptance criteria

- [ ] At least one simplification was applied (or the PR
      description explicitly states "no further simplification
      found, sweep complete").
- [ ] Every file in the list above was opened and read top-to-bottom.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.
