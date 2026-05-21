# Task: NixOS VM test update for the new runtime API

Parent: `600_story_runtime_api_redesign.md`

## Scope

Update the existing NixOS VM test (`nix build .#checks.x86_64-linux.vm-test`)
to drive the new three-endpoint runtime API end-to-end. Assert that no
SSH-approval code paths remain, that the env-immutability rule holds
through the full stack, and that the daemon survives a backend
restart without manual intervention.

### Existing assertions to preserve

The current VM test ships 60+ assertions covering:

- Local and remote-style instance shapes.
- Unprivileged-user invocation, spoofing resistance, PID namespace
  isolation.
- Memory protection (`PR_SET_DUMPABLE=0`), core dump prevention.
- Grant-driven wrapper locking.
- Always-rw mount + writeback warning.

All of these stay. The diffs are confined to the runtime-API call
shapes and the SSH-approval coverage.

### New assertions

In `nix/tests/test.py`:

- **Backend restart self-heal**:
  - With the daemon running and a successful wrapper read on
    record, `systemctl restart underwrap-backendd` on the backend
    host.
  - Wait for the unit to be Active.
  - Run the same wrapper invocation again from the unprivileged
    user. Assert it succeeds (no "not enrolled" error, no manual
    daemon restart).

- **Env immutability through the full stack**:
  - Create a resource with one env var (e.g. `TOKEN=abc`).
  - Wrapper run that target writes a new value via libsecret to the
    same resource (which won't touch env), then exits. Checkin
    succeeds. Backend blob unchanged in env.
  - Manually construct a writeback attempt (via the debug tool or
    a small Python helper inside the VM) where the env section is
    mutated. Assert the wrapper sees the writeback-rejected
    journal event, and `journalctl --grep` for the
    `env_section_immutable` audit line.

- **Per-call approval no longer exists**:
  - Attempt to POST a policy document with a `PerCallApproval` mode
    via the admin API. Assert 400 with a "unknown variant" / schema
    error.
  - `grep` the running daemon binary's strings for `approval_key`,
    `ApprovalProof`, `ApprovalRequired`. Assert no hits. (Use
    `nix build` artefact; this is a build-output assertion.)

- **No `secret_id` field reachable**:
  - Attempt a POST to `/v1/write` with crafted JSON. Assert 404
    (route no longer exists).

### Spoofing test update

The existing `nix/tests/spoof.py` exercises socket spoofing against
the wrapper sockets. It doesn't touch the HTTPS API, so it should
keep working unchanged. **Verify** during the task that no spoof
test sends frames against legacy IPC verbs that were dropped.

### Acceptance criteria

- [ ] `nix build .#checks.x86_64-linux.vm-test` green on a clean
      checkout.
- [ ] The backend-restart-self-heal assertion explicitly tests the
      regression the immediate fix addressed.
- [ ] The env-immutability assertion goes through the full
      wrapper→namespace→daemon→backend path, not just an HTTP call.
- [ ] The "no per-call approval anywhere" assertion includes both a
      live API attempt and a binary-strings grep.

### Out of scope

- Rebuilding the VM test from scratch. This is a delta on the
  existing test, not a rewrite.
