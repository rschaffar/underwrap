# Task: NixOS VM test covering locked stores, unlock, and rotation

Parent: `500_story_in_memory_master_keys.md`
Epic: `epic_in_memory_master_keys.md`

## Scope

Add a scenario to `nix/tests/test.py` (or split into a sibling file
if the VM-test layout already prefers one-scenario-per-file):

1. Start backend + frontend with one encrypted store.
2. Create the store via the admin HTTPS API; record the returned
   `key_hex`.
3. Seed a value into a known resource through the runtime path
   (existing helper).
4. Read it once via a wrapper — must succeed.
5. `systemctl restart underwrap-backendd`. Wait for readiness.
6. Wrapper read — must fail with the `store_locked` error code and
   the wrapper's stderr line must match
   `underwrap: store '<name>' is locked`.
7. POST `/v1/admin/stores/{id}/unlock` with the recorded hex.
8. Wrapper read — must succeed and return the same plaintext as
   step 4.
9. POST `/v1/admin/stores/{id}/lock`. Wrapper read — must fail
   again.
10. Unlock again. POST `/v1/admin/stores/{id}/rotate-key`. Capture
    the new hex.
11. Wrapper read — must still succeed with the same plaintext.
12. `systemctl restart underwrap-backendd`. Unlock with the **new**
    hex (the old one must fail with `key_mismatch`).
13. Wrapper read — must still succeed.

Add a negative sub-test:

- Unlock with `00`*32 against a fresh encrypted store → 403 +
  `key_mismatch`.
- Unlock with `XYZ...` (invalid hex) → 400 + `invalid_key_hex`.
- Unlock against a plain store → 400 + `store_not_encrypted`.

`nix/tests/spoof.py` does not need changes for this ticket; the
identity model is unaffected.

## Acceptance criteria

- [ ] `nix build .#checks.x86_64-linux.vm-test` green with the new
      scenario.
- [ ] The scenario fails meaningfully if any of the
      `ErrorCode::StoreLocked` translations regress (verified by
      mutation: comment out the registry guard, observe the VM test
      catches it).

## Out of scope

- Cross-host failover, key escrow, or HA topologies — not part of
  the project.
