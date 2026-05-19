# Task: VM end-to-end test for atomic blob writeback and rejection

Parent: `430_story_atomic_blob_documentation_and_vm_cutover.md`

## Scope

Add or extend the NixOS VM test
(`nix/tests/test.py`, `nix/tests/node.nix`) so it exercises the
full atomic-blob loop against a real `underwrap-backendd` and a
real wrapper inside the VM.

### Test scenarios

The VM test already boots real services and runs assertions in
Python; add the following scenarios with appropriate isolation:

1. **Clean run with no changes**
   - Wrapper invoked with libsecret + files in scope.
   - Target reads but does not write.
   - Assert: backend blob bytes unchanged byte-for-byte after the
     run.
   - Assert: pair `last_used_at` updated.
   - Assert: journal contains no `writeback_rejected` event.

2. **Changed run with writable grant — Checkin path**
   - Same wrapper; target performs a libsecret CreateItem and
     writes a file into the bound directory.
   - Assert: backend blob now contains both mutations.
   - Assert: a second wrapper run sees them via Checkout.

3. **Changed run with read-only grant — quiet drop**
   - Same wrapper; grant `writable=false` for the resource.
   - Target performs the same mutations.
   - Assert: backend blob unchanged.
   - Assert: journal contains exactly one `writeback_rejected`
     event with `reason=grant_readonly`.
   - Assert: wrapper stderr contains the human-readable line.

4. **Suppression env var**
   - Same as (3) but with
     `UNDERWRAP_SUPPRESS_READONLY_WARNING=1` set when launching
     the wrapper.
   - Assert: journal still contains the event.
   - Assert: wrapper stderr does not contain the human-readable
     line.
   - Assert: target process did not see the env var (check via a
     target that prints its env).

5. **Always-writable namespace mount**
   - Read-only grant; target performs `touch /home/.../file`
     inside the bound directory.
   - Assert: the write succeeds (target's exit code is 0).
   - Assert: the file does not persist to the backend (covered by
     scenario 3's persistence check).

6. **Server-side reject on grant flip**
   - Wrapper Checkouts a writable grant; admin flips the grant to
     read-only while the target is running; target performs
     mutations; wrapper exits.
   - Assert: backend blob unchanged.
   - Assert: journal contains `writeback_rejected` event with
     `reason=server_rejected`.

7. **Spoofing resistance** (regression)
   - Existing spoof.py path still passes: an unprivileged process
     can't directly send Checkin to the daemon channel.

### Implementation notes

- Add wrapper config that exposes libsecret + files in scope to a
  small helper binary the test can drive (existing
  `frontend-keyring-probe` is a starting point but probably needs
  a sibling binary that also writes a file).
- Use the existing audit/journal capture pattern in test.py.

## Acceptance criteria

- [ ] `nix build .#checks.x86_64-linux.vm-test` green with the new
      scenarios.
- [ ] Each scenario has at least one explicit Python assertion.
- [ ] Test count in CLAUDE.md ("21 assertions" today) updated to
      match the new total.

## Out of scope

- Performance benchmarking.
- Multi-namespace concurrent-wrapper races (covered by integration
  tests in 405 / 415 with deterministic barriers).
