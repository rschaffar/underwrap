# Task: Remove dead `installed_key` placeholder + simplify `stage_name` suffix

Parent: `1000_story_backend_backendd_refinement.md`
Resolves: F-003, F-004 (review report `backend-backendd_2026-05-21.md`)

## Background

Two minor leaks in `handle_admin_store_rotate_key`
(`crates/backend-backendd/src/admin_api/stores.rs`):

1. Lines 862-863 generate 32 bytes from the OS RNG into a local
   `installed_key` that is immediately discarded:

   ```rust
   let installed_key = generate_master_key_bytes(); // placeholder, immediately overwritten
   let _ = installed_key; // keep clippy happy if it ever sees an unused let
   ```

   The "placeholder, immediately overwritten" never happens — nothing
   below assigns to `installed_key` or reads it. Wastes entropy and
   reads as a footgun.

2. Lines 778-785 build a 16-character random suffix for the staging
   directory by calling
   `master_key_hex(&generate_master_key_bytes()).chars().take(16)`.
   That allocates a 32-byte master-key-grade hex string only to lop
   off all but 16 chars, and uses helpers whose names imply
   "master key material" for a non-cryptographic disambiguator.

## Scope

In `crates/backend-backendd/src/admin_api/stores.rs`:

- Delete the two-line `installed_key` block at 862-863.
- Add a small `random_hex_8() -> String` helper next to
  `generate_master_key_bytes` (or `random_hex(byte_count: usize)` if
  ticket 1040 has landed first — see note below). Use it in
  `stage_name`.
- Replace the multi-line `chars().take(16).collect::<String>()`
  expression with the helper call so the format string is one
  argument.

## Coordination with 1040

If ticket 1040 (extract `random_id` / random helpers) lands first,
this task should use whatever helper that ticket exposed instead of
adding a duplicate. If 1030 lands first, 1040 will absorb the
`random_hex_8` helper into its broader random-helpers module.

## Out of scope

- The rest of `handle_admin_store_rotate_key`'s error flow. The
  rollback-on-rename-failure logic is correct; this is not the
  ticket to overhaul it.
- The rotation `Mutex` map (`AdminRuntimeState::rotation_locks`).

## Acceptance criteria

- [ ] `grep -n "installed_key\|placeholder, immediately overwritten"
       crates/backend-backendd/src/admin_api/stores.rs` returns no
      matches.
- [ ] `stage_name` no longer calls `master_key_hex` or
      `generate_master_key_bytes` (it uses the random-hex helper
      added here or in 1040).
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes (the rotate-key
      integration / unit tests are the load-bearing safety net).
