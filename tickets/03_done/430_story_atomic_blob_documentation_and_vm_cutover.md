# Story: Documentation and VM cutover for atomic resource blobs

Parent: `epic_atomic_resource_blobs.md`

## Outcome

After this story, the new model is the documented model. `security.md`,
`architecture.md`, `backend.md`, the example config, and `CLAUDE.md`
describe one-blob-per-resource, the three new IPC verbs, the
always-writable namespace mount, the server-side write gate, the
suppression env var, and the in-memory residency of secret material in
secretsd. The NixOS VM test covers a real end-to-end Checkout/Checkin
loop, last-used updates, and a read-only-grant rejection path.

## Tasks

- `431_task_docs_update_security_architecture_backend.md`
- `432_task_vm_test_atomic_blob_e2e_and_writeback_rejection.md`

## Acceptance criteria

- [ ] `docs/security.md` "Design invariants" section notes the
      full-blob residency in secretsd memory and the rationale
      (PR_SET_DUMPABLE=0, namespace isolation).
- [ ] `docs/architecture.md` and `docs/backend.md` describe the new
      storage layout and protocol. The old per-key per-slot picture
      is removed.
- [ ] `examples/config.yaml` notes that the namespace directory mount
      is always rw and that the server gates writeback.
- [ ] `CLAUDE.md` lists the new IPC verbs, the env var, and adds
      `UNDERWRAP_SUPPRESS_READONLY_WARNING` to the wrapper env section.
- [ ] `nix build .#checks.x86_64-linux.vm-test` green and exercises:
      unchanged run → only UsageSignal; changed + writable → Checkin;
      changed + read-only → journal entry + stderr warning + no
      backend change; suppression env var hides the stderr line;
      namespace mount is rw even on a read-only grant.
