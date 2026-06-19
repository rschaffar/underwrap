# Task: Docs + VM tests for per-(instance, wrapper) sandbox uids

Parent: `2720_story_per_wrapper_target_uid`
Depends on: `2730_task_nix_per_wrapper_target_uid`,
`2740_task_runtime_isolation_key_instance_wrapper`

## Problem

The path-keyed scheme is documented as a design invariant in several places and
asserted in the VM test. After `2730`/`2740` re-key on `(instance, wrapper)`,
the docs and tests must describe and *prove* the new model — including the two
properties that motivated the change (cross-instance separation, no shared debug
uid).

## Fix — documentation

- `docs/security.md`:
  - "Which UID does the target run as?" (~`:218`): the uid comes from the
    root-owned table "keyed on the daemon-validated `(instance, wrapper)`
    selector" (was "daemon-chosen target path").
  - "Protecting the final target process" (~`:556`–`622`, `:707`–`732`): each
    `(instance, wrapper)` gets its own dedicated uid; **state explicitly** that
    two wrappers sharing a binary, and the same binary across two instances, get
    **distinct** uids — so the uid itself separates distinct secrets and the
    "fully isolated instances" claim holds at the uid layer. Replace any "same
    tool = same trust domain" wording with the `(instance, wrapper)` rationale.
  - Debug: note the `Debug` variant runs under its wrapper's uid (no host-wide
    debug-tool uid).
  - Yama section (~`:750`): the per-secret uid boundary is now the primary
    separation between distinct secrets; Yama stays as defense-in-depth.
- `docs/architecture.md` (~`:222`–`241`): runtime-dir mechanism is unchanged
  (keyed on the target user *name* via `getpwuid`), but the name now encodes
  `(instance, wrapper)`. Adjust wording.
- `docs/testing.md` (~`:68`–`76`): update the target-uid coverage description to
  the `(instance, wrapper)` model and the new assertions below.
- `examples/config.yaml` (~`:39`–`43`): the `target_runtime_dir_base` comment
  still derives the dir from `<target-user-name>` — confirm wording still reads
  correctly (the name is now per wrapper); tweak only if misleading.

Do **not** edit the historical done tickets (`2600`/`2620`/`2630`/etc.); their
forward-references to per-path keying are provenance. The `2720` story records
that this work supersedes the "finer granularity is `2670`" pointer.

## Fix — VM test (`nix/tests/`)

- `node.nix`: add fixtures that make the new properties testable:
  - two isolated wrappers in one instance pointing at the **same** target binary
    (distinct resources), and
  - a wrapper in a **second** instance pointing at the **same** binary as a
    first-instance wrapper.
  Reuse existing isolated fixtures where possible; add only what proves
  distinctness.
- `test.py`, `local_target_uid_authority()` (~`:1500`–`1530`): update the table
  parser to 4 columns `<instance>\t<wrapper>\t<user>\t<group>`; assert
  `user.startswith("underwrap-target-")`, column 1/2 are non-empty, user is a
  member of only its own primary group, no row for the debug tool.
- Add assertions proving the **distinctness** properties:
  - the two same-binary wrappers resolve to **different** uids,
  - the same binary across the two instances resolves to **different** uids,
  - a `Debug` invocation of wrapper W runs as W's sandbox uid (read
    `/proc/<pid>/status` from root, compare to the table row for
    `(instance, W)`), and that uid is **not** shared with any other wrapper.
- Keep the existing caller-cannot-read-the-secret matrix green — the
  secret-from-caller boundary is unchanged.

## Acceptance

- Docs describe the `(instance, wrapper)` model with no stale "same tool = same
  trust domain" / path-keyed wording, and assert cross-instance + cross-secret
  uid distinctness.
- VM test proves: distinct uids for same-binary wrappers, distinct uids across
  instances, debug-under-wrapper-uid, table format/ownership, group membership.
- `grep` across `docs/` and `nix/tests/` finds no remaining reference to the
  path-keyed table format.
- `nix build .#checks.x86_64-linux.vm-test` green (confirm via PASS lines in the
  driver log, not the exit code alone).

## Out of scope

- The implementation itself (`2730`/`2740`).

## Status: DONE (2720)

Docs re-keyed to (instance, wrapper): security.md ("Which UID" + "Mandatory
dedicated target UID" sections describe per-pair keying, distinctness, and the
selector-not-uid trust posture; removed the now-false "Per-wrapper target UIDs
... rejected" row), architecture.md (runtime dir is per pair), testing.md
(per-pair coverage + distinctness). examples/config.yaml left as-is.

VM test: reused existing fixtures that already share a binary (local/test-echo
+ local/test-locked on `env`; tunnel/remote-echo on `env`).
local_target_uid_authority() parses the 4-column table and asserts: same binary
within an instance -> distinct uids; same binary across instances -> distinct
uids; no uid shared across pairs. local_debug_subcommand() asserts the Debug
variant runs under test-echo's sandbox uid (980), not a host-wide debug uid.

nix build .#checks.x86_64-linux.vm-test GREEN: 105 PASS lines, 0 AssertionError,
confirmed by scanning the driver log (the exit code was masked by a tee/`; echo`
pipeline on the first run, which surfaced a test-only wrong-instance bug:
remote -> tunnel).
