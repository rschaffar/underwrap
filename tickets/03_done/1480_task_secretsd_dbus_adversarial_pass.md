# Task: Adversarial pass on the secretsd D-Bus surface

Part of: `epic_frontend_security_hardening_2026-05`.

## Problem

`underwrap-secretsd` is the one frontend component where
target-supplied bytes get parsed by our own code. Everything else
on the wrapper-chain side speaks our own IPC protocol over fds we
control; secretsd accepts D-Bus method calls from the target
process, which is the most potentially-hostile process in the
system (it ran the target binary and may have been compromised).

zbus is a mature library and the D-Bus protocol itself is
well-trodden, so the realistic risk is not "a parser bug in zbus"
but "a logic bug in our dbus_service handlers". Specifically the
`Service`, `Collection`, and `Item` interfaces in
`crates/frontend-secretsd/src/dbus_service/`. Things to look at
deliberately:

1. **Path traversal in object-path handling.** D-Bus object paths
   like `/org/freedesktop/secrets/aliases/<alias>` carry an alias
   string. What happens if the alias contains characters that
   round-trip through serialisation or get interpreted as paths
   elsewhere? Our `BASE_PATH` formatting in
   `frontend-secretsd/src/main.rs:226` does not validate. zbus's
   `ObjectPath::try_from` rejects clearly-illegal paths, but the
   validation in our own data flow (state file, JCS canonical bytes)
   is a separate question.

2. **Resource-exhaustion via large session secrets.** The
   `Service::OpenSession` flow accepts client-supplied bytes
   (`Variant`s) for DH key exchange. Unbounded sizes? Memory
   pressure as a DoS against the namespace's RAM budget?

3. **The state-file write path.** Every D-Bus mutation writes JCS
   bytes atomically to `--state-path`. A target that triggers many
   mutations in a tight loop could exhaust the staging tmpfs
   (`size=4m`) or just create high IO churn. Worth verifying the
   tmpfs size limit is in fact enforced and the wrapper aborts
   cleanly when full, rather than silently dropping mutations.

4. **The Item.Delete + concurrent Collection.SearchItems pattern.**
   Any race that surfaces a stale Item path or returns a Variant
   pointing at a freed/cleared secret.

5. **The libsecret section JCS canonicalisation.** We canonicalise
   on writeback (`writeback.rs`). What does secretsd do if the
   target writes a value that fails canonicalisation later? Does
   the failure mode lose data, or surface as an audit event and
   refused writeback?

6. **The init-fd consumption.** secretsd reads the initial libsecret
   section from `--init-fd` (`crates/frontend-secretsd/src/main.rs:
   --init-fd` arg). Bounded read, or could a malformed input from a
   compromised ns hang it indefinitely?

None of these are known bugs. This ticket is a deliberate
adversarial review of the surface before we promote libsecret mode
as the recommended path (which the broader hardening epic should do).

## What to do

1. **Read-through pass.** Walk
   `crates/frontend-secretsd/src/dbus_service/` with adversarial
   intent. For each method handler in `Service`, `Collection`, and
   `Item`, write a short note: what does this assume about its
   inputs, and what happens if those assumptions are violated.
   Look specifically for:
   - unbounded allocation on input size
   - unchecked indexing / string slicing
   - state-machine transitions that assume a particular call order
   - error paths that silently succeed or lose data

2. **Property tests.** Add property tests under
   `frontend-secretsd/src/dbus_service/tests.rs` (sibling-tests
   layout per CLAUDE.md) for the JCS round-trip on
   target-modifiable fields. Invariant: any value that secretsd
   accepts via D-Bus must canonicalise without error.

3. **Fuzz target.** Add a `cargo-fuzz` or `arbitrary`-based fuzz
   target for the D-Bus message parsing path that exercises the
   handlers. Even a few minutes of fuzzing on the JCS write path
   covers a lot of ground. Wire into CI as a smoke run, not a
   blocking gate.

4. **Resource limits.** Verify (or add) that secretsd refuses
   writes once the state file would exceed a sane size limit
   (suggest 1 MiB by default — well under the 4 MiB tmpfs and
   far more than any real libsecret keyring needs). The cap
   should be configurable via secretsd CLI arg, defaulted by
   ns, surfaced in the wrapper config.

5. **Write up findings.** Output of step 1 lands in a short
   `docs/secretsd-threat-notes.md` (operator-facing) or as
   inline `// SAFETY:` / `// INVARIANT:` comments next to the
   handlers, depending on what fits the codebase style. The
   point is that the assumptions are documented somewhere the
   next reviewer will find.

## Acceptance

- Notes document or inline annotations present for every method
  handler in `Service`, `Collection`, `Item`.
- Property tests for JCS round-trip on target-modifiable fields.
- A fuzz target exists and runs cleanly for at least 5 minutes
  without finding a crash.
- Secretsd state-file size cap enforced and tested.
- `cargo test --workspace` green.
- `cargo clippy --all-targets --workspace -- -D warnings` green.
- VM test green.

## Note

This ticket is defence-in-depth, not closing a known bug. It is
the right shape of work to do *before* recommending libsecret mode
as the default path for new wrappers, since libsecret mode shifts
trust from "Yama protects the target's environ" to "secretsd
correctly mediates D-Bus method calls from the target." That trust
shift is the right one to make, but it deserves a deliberate
review of the new surface.
