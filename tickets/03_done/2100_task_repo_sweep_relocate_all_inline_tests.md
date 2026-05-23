# Task: Repo sweep — relocate every inline `mod tests {}` block to a sibling `tests.rs`

## Background

`AGENTS.md` (test file layout section) requires the sibling-file
submodule pattern for Rust tests — production files never carry
an inline `#[cfg(test)] mod tests { ... }` block. The previous
"tiny exception" judgement language was removed in the same
change as this ticket's review; the rule is now unconditional.

Source review:
`tickets/_review/web_admin_spa_2026-05-23.md` F-016.

## Scope

Move every inline `mod tests { ... }` block in the workspace to a
sibling `tests.rs`. The list:

| File | Inline mod tests at | Approximate length |
| --- | --- | --- |
| `crates/backend-backendd/src/main.rs` | line 119 | 27 lines |
| `crates/frontend-secretsd/src/dbus_service.rs` | line 98 | 16 lines |
| `crates/frontend-exec/src/main.rs` | line 464 | 19 lines |
| `crates/shared-protocol/src/message.rs` | line 16 | 24 lines |
| `crates/shared-protocol/src/service.rs` | line 587 | 15 lines |
| `crates/frontend-underwrapd/src/main.rs` | line 60 | 26 lines |
| `crates/frontend-underwrapd/src/storage.rs` | line 6 | 14 lines |
| `crates/frontend-underwrapd/src/lib.rs` | line 12 | 9 lines |

(Verify the list by re-running
`rg -n '^#\[cfg\(test\)\][[:space:]]*$' crates/ -A 1 | rg
'^mod tests \{'` before starting; if anything has landed in
between, fold the new hits into this ticket too.)

### Mechanical procedure per file

For each production file `foo.rs`:

1. Create `foo/` directory if it doesn't already exist.
2. Move the entire `mod tests { ... }` body into
   `foo/tests.rs` as the file contents.
3. The new `tests.rs` starts directly with `use super::*;` and
   the test functions. No `mod tests { ... }` wrapper, no
   `#[cfg(test)]` attribute (the declaration in the parent
   handles it).
4. In the production file `foo.rs`, replace the inline block
   with `#[cfg(test)] mod tests;` at the same position (the very
   end of the file).
5. If the inline block referenced module-private items by name,
   they're still in scope via `use super::*;` — verify
   `cargo build` and `cargo test -p <crate>` for the touched
   crate.

### Special cases

- **`*/main.rs` files** (backend-backendd, frontend-exec,
  frontend-underwrapd): the sibling path is `src/main/tests.rs`
  inside a new `src/main/` directory. Note that some
  filesystems are case-sensitive — pick the lowercase form. The
  parent `main.rs` already declares `mod tests;` at the bottom
  in place of the inline block.
- **`lib.rs` files** (frontend-underwrapd): sibling path is
  `src/lib/tests.rs` under a new `src/lib/` directory; same
  pattern.
- **`storage.rs`** (frontend-underwrapd, 14 lines, prod module is
  5 lines): straight relocation. The resulting `storage/tests.rs`
  is small but matches the rule.

### Helper hoist (only when both `tests.rs` siblings already share fixtures)

If during the move you notice the same fixture being hand-rolled
in more than one of these `tests.rs` files (e.g. an identical
`MockX` or `temp_dir_with_*` helper), hoist it into
`src/test_support.rs` for that crate following the
AGENTS.md-documented pattern. Do not invent new fixtures, only
deduplicate ones that already exist.

## Out of scope

- Changing what any test actually asserts.
- Splitting any `tests.rs` files that exceed the ~600-line
  guideline (those would be their own tickets).
- Adding new tests.

## Acceptance criteria

- [ ] `rg -n '^mod tests \{$' crates/` returns zero hits (no
      production file carries an inline tests block).
- [ ] Every parent file in the table above ends with
      `#[cfg(test)] mod tests;`.
- [ ] Every corresponding sibling `tests.rs` exists and is
      reachable (its tests run on `cargo test -p <crate>`).
- [ ] `cargo test --workspace` passes (same test count as before
      the move, plus or minus zero).
- [ ] `cargo clippy --all-targets --workspace -- -D warnings`
      passes.
- [ ] `cargo fmt -- --check` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes (sanity
      guard for any cross-crate fallout).

## Notes for the implementer

- Pure moves only. If you find that a test relied on something
  the inline block had visibility to but the sibling does not,
  the AGENTS.md sibling-file rule says use a direct `use crate::<path>;`
  from where the item lives — do *not* add
  `#[cfg(test)] pub(crate) use ...` re-exports at the crate
  root. (That re-export shim is forbidden by AGENTS.md.)
- The eight files are independent. Land in one PR if you want a
  single review; split into per-crate PRs if any individual move
  ends up larger than expected.
