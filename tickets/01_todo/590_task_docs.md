# Task: Documentation sweep for in-memory master keys

Parent: `500_story_in_memory_master_keys.md`
Epic: `epic_in_memory_master_keys.md`

## Scope

- `docs/backend.md`:
  - Rewrite the "Encrypted stores" / key-material section.
    - Keys live in process memory only.
    - `create-store` returns the hex once; the operator is solely
      responsible for storing it.
    - Restart strands all encrypted stores until an operator
      unlocks them via the admin API.
    - `rotate-key` re-encrypts every resource and returns a fresh
      hex once.
  - Update the admin API reference to add `POST .../unlock` and
    `POST .../lock`, including request bodies and the structured
    error codes (`store_locked`, `key_mismatch`,
    `invalid_key_hex`, `store_not_encrypted`, `already_unlocked`).
  - Remove the reference to `keys_dir` / on-disk key files
    everywhere.
- `docs/security.md`:
  - Update the "Design invariants" entry for at-rest encryption to
    state that the master key is never written to disk and that
    process restart is the lock event.
  - Note the explicit trust model: an attacker with read access to
    the data directory cannot recover plaintext without the
    operator's key. An attacker with memory-dump access to the
    backend process can — out of scope for this design.
- `examples/config.yaml`: if it references `key_file`, remove it.
- `CLAUDE.md` (project root): replace any mention of `keys_dir` or
  on-disk master keys in the architecture / security-critical
  sections with the in-memory model. Add a brief note under "Always
  do" that on backend restart, encrypted stores remain locked
  until an operator unlocks them.
- `nix/modules/underwrap-backend.nix`: remove any `keysDir`
  option / documentation (it should be gone after 550). Double-check.

## Acceptance criteria

- [ ] No doc mentions `keys_dir` or on-disk key files for the
      backend other than as a "this used to exist" historical note,
      if at all.
- [ ] The unlock and lock endpoints are documented with request /
      response shapes and error codes.
- [ ] `CLAUDE.md` reflects the new invariant.

## Out of scope

- Generated reference docs / RustDoc rebuilds beyond what `cargo
  doc` produces automatically.
