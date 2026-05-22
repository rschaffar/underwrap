# Task: Repo sweep — relocate every over-threshold inline `mod tests {}` block

Parent: `1300_story_backend_backendd_refinement.md`
Resolves: F-018 (review report `backend_backendd_2026-05-22.md`)

## Background

`CLAUDE.md` mandates the sibling-file `tests.rs` submodule pattern,
with a tiny exception for "one or two tiny tests where a separate
file would be ceremony for five lines." The skill's threshold for
the exception is ~30 lines.

Ticket 1080 (closed) split the backend-backendd-side inline blocks.
Ticket 1210 (closed) swept three more files where tests substantially
exceeded production. Many over-threshold inline blocks still exist
across the rest of the workspace — counted in finding F-018.

## Scope

For each file below, move the inline `#[cfg(test)] mod tests { ... }`
block into a sibling `<module>/tests.rs` (or `<file>/tests.rs` if the
file is the directory's `mod.rs`). Replace the inline block with:

```rust
#[cfg(test)]
mod tests;
```

Production file order, longest inline block first:

- `crates/shared-protocol/src/resource_blob.rs:133` (256 lines).
- `crates/shared-protocol/src/entities.rs:248` (239).
- `crates/shared-protocol/src/config.rs:198` (185).
- `crates/frontend-keygen/src/lib.rs:291` (152).
- `crates/shared-store/src/path.rs:160` (128).
- `crates/shared-protocol/src/policy.rs:176` (102).
- `crates/shared-protocol/src/ipc.rs:322` (100).
- `crates/shared-protocol/src/exec.rs:83` (92).
- `crates/shared-store/src/encrypted.rs:114` (85).
- `crates/shared-protocol/src/resource.rs:213` (82).
- `crates/frontend-underwrapd/src/audit.rs:135` (80).
- `crates/frontend-secretsd/src/dbus_service/service.rs:224` (72).
- `crates/frontend-secretsd/src/main.rs:262` (68).
- `crates/frontend-secretsd/src/dbus_service/item.rs:177` (66).
- `crates/frontend-secretsd/src/section_store.rs:122` (65).
- `crates/frontend-secretsd/src/state.rs:82` (64).
- `crates/frontend-ns/src/env.rs:70` (63).
- `crates/shared-store/src/lib.rs:130` (61).
- `crates/shared-protocol/src/message/wrapper_resource.rs:143` (60).
- `crates/shared-protocol/src/scm_rights.rs:119` (50).
- `crates/frontend-underwrapd/src/server.rs:252` (46).
- `crates/frontend-underwrapd/src/backend_client.rs:322` (45).
- `crates/frontend-ns/src/libsecret.rs:196` (45).
- `crates/frontend-keyring-probe/src/main.rs:249` (41).
- `crates/frontend-underwrapd/src/startup.rs:170` (38).
- `crates/integration-tests/src/harness/builders.rs:55` (37).
- `crates/frontend-ns/src/directory_credential.rs:303` (35).
- `crates/frontend-secretsd/src/dbus_service/collection.rs:179` (35).

Plus this crate's binary entry point:

- `crates/backend-backendd/src/main.rs:169` (118 lines). Binaries
  can't use the sibling-submodule trick directly — move
  `parse_backend_config` and `recover_user` into the library crate
  (`crates/backend-backendd/src/lib.rs` re-exports, or new modules
  like `crate::cli`), and put their tests in a sibling `tests.rs`
  next to wherever they land. Update `main.rs` to call into the
  library. The remaining smoke tests in `main.rs` (`Cli` parse
  tests) are sub-threshold and can stay inline.

### Sub-threshold blocks — do NOT touch

These sit under the ~30-line tiny exception and stay inline:

- `crates/frontend-underwrapd/src/main.rs:59` (28).
- `crates/shared-protocol/src/message.rs:19` (26).
- `crates/frontend-exec/src/main.rs:463` (21).
- `crates/frontend-secretsd/src/dbus_service.rs:100` (18).
- `crates/shared-protocol/src/service.rs:850` (17).
- `crates/frontend-underwrapd/src/storage.rs:5` (16).
- `crates/frontend-underwrapd/src/lib.rs:11` (11).

### Mechanical rules per file

1. Move the entire body between `mod tests {` and the matching `}`
   into the new sibling — **without** the wrapper itself.
2. Imports inside the moved tests stay as-is. `use super::*;` keeps
   working because the new file is a submodule of the production
   file's module.
3. Do not refactor the moved tests.
4. Do not consolidate shared fixtures across multiple `tests.rs`
   files into a `test_support.rs` in this ticket. That's a separate
   refinement decision.

## Out of scope

- Refactoring the moved tests themselves.
- Splitting `tests.rs` files further (none of the new files should
  hit the 600-line ceiling — verify with `wc -l` after each move).
- Adding `test_support.rs` modules.

## Acceptance criteria

- [ ] None of the production files in the "Scope" list above
      contains an inline `#[cfg(test)] mod tests {` block.
- [ ] Each of those files ends with `#[cfg(test)] mod tests;`.
- [ ] The sub-threshold files in the "do NOT touch" list still
      have their inline blocks.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.
