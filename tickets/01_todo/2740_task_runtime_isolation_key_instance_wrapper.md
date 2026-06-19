# Task: Resolve the sandbox uid by (instance, wrapper) in underwrap-ns

Parent: `2720_story_per_wrapper_target_uid`
Depends on: `2730_task_nix_per_wrapper_target_uid`
Blocks: `2750_task_docs_and_vm_tests_per_wrapper_uid`

## Problem

`underwrap-ns` resolves the sandbox uid by looking up the daemon-chosen target
**path** in `/etc/underwrap/target-uids`
(`crates/underwrap-ns/src/target_uid.rs:66`). After `2730` re-keys the table on
`(instance, wrapper)`, the lookup must use that pair instead. The pair must
reach `underwrap-ns` from a trusted source without adding a uid/gid to the
wire.

## Fix

### Source the selector in `underwrap-exec` (no protocol change)

`underwrap-exec` already holds both halves authoritatively:

- instance — `instance_from_egid()` (`crates/underwrap-exec/src/main.rs:148`),
  derived from the kernel-enforced effective GID gate.
- wrapper — `Invocation.wrapper_name` from `parse_invocation()`
  (`:121`), the name the daemon already validated for this very read.

Pass both to `underwrap-ns` as internal env vars alongside the existing
`UNDERWRAP_LIBSECRET` plumbing (`:404`–`417`):

- `UNDERWRAP_ISOLATION_INSTANCE`
- `UNDERWRAP_ISOLATION_WRAPPER`

`env_remove` them first (defense against a caller pre-setting them), then set
them. **Never propagate them into the target** — `underwrap-ns` removes them
before the target exec, same discipline as
`UNDERWRAP_SUPPRESS_READONLY_WARNING` / the libsecret vars.

This keeps `ExecBundle` and the daemon read response unchanged. The
`exec.rs` authority invariant (`:52`–`62`) still holds — we relay a
*selector* (resolved against the root-owned table), never a uid/gid. Update
that doc comment to say the identity is "resolved … from the root-owned
`/etc/underwrap/target-uids` table, keyed on the daemon-validated
`(instance, wrapper)` selector" instead of "the daemon-chosen target path."

### Re-key the lookup in `target_uid.rs`

- `resolve_isolation(target_path)` → `resolve_isolation(instance, wrapper)`.
  Drop `target_path` from the signature; the path no longer participates in uid
  selection. Read `(instance, wrapper)` from the two env vars at the
  `main.rs:82` call site; **hard-error (no fallback)** if either env var is
  missing or empty — mandatory isolation, consistent with the existing
  missing-table / missing-row hard errors.
- `resolve_isolation_from` (`:41`): parse four tab-separated columns
  `<instance>\t<wrapper>\t<user>\t<group>`; match when columns 1 and 2 equal the
  requested pair; resolve columns 3 and 4 via the existing
  `resolve_uid`/`resolve_gid` + the floor / `gid != 0` checks (unchanged).
  Update the "more than 3 fields" malformed-row guard (`:61`) to "more than 4."
- Update the module doc (`:1`–`17`) and the missing-row / missing-table error
  strings to name the `(instance, wrapper)` pair instead of the path.
- `user_name(uid)` / `with_target_fsuid` / `switch_to` / `verify_switched` are
  unchanged — they operate on the resolved `Isolation { uid, gid }`, and the
  runtime-dir derivation still flows from `getpwuid(X).pw_name`.

### Tests (`crates/underwrap-ns/src/target_uid/tests.rs`)

- Rewrite fixture tables to the 4-column format; cover: exact `(instance,
  wrapper)` match; same wrapper name under a *different* instance resolving to a
  *different* row (the core property); missing row → error; malformed (5-column)
  row → error; uid floor / gid-0 rejection still fires.
- `crates/underwrap-exec` tests: assert both `UNDERWRAP_ISOLATION_*` vars are
  set on the ns command and that the target environment never contains them.

## Acceptance

- `underwrap-ns` selects the uid solely from `(instance, wrapper)`; the target
  path is used only to exec, never for uid selection.
- Missing/empty `UNDERWRAP_ISOLATION_*` env aborts the run (no caller fallback).
- The two env vars never appear in the target's environment.
- `ExecBundle` round-trip test still confirms no identity field
  (`exec_bundle_carries_no_target_identity` unchanged).
- `cargo test -p underwrap-ns -p underwrap-exec -p underwrap-protocol`,
  `cargo clippy --all-targets --workspace -- -D warnings`, `cargo fmt --check`
  green.

## Out of scope

- NixOS table/user generation (`2730`).
- VM-test matrix + docs (`2750`).
