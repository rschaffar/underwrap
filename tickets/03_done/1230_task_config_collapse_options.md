# Task: Collapse `Option<T>` config fields to required + serde default

Resolves: B-CFG-1, B-CFG-2, B-CFG-3.

## Problem

`crates/backend-backendd/src/config.rs` carries several `Option<T>`
config fields that are always resolved to a default by their consumer.
The `Option` adds nothing — empty/missing and explicitly-defaulted
are operationally identical.

Sites:

1. **`allowed_origins: Option<Vec<String>>`** (B-CFG-1):
   `crates/backend-backendd/src/config.rs:28-29`. Consumer at
   `crates/backend-backendd/src/http/origin.rs:46-49` collapses
   `None` and `Some(vec![])` to the same "no allowlist" path.

2. **`session_lifetime_secs: Option<u64>`** (B-CFG-2):
   `crates/backend-backendd/src/config.rs:32-33`. Consumer at
   `crates/backend-backendd/src/server.rs:63-65` does
   `.unwrap_or(DEFAULT_SESSION_LIFETIME_SECS)`.

3. **Four `Option<PathBuf>` dirs** (B-CFG-3):
   `crates/backend-backendd/src/config.rs:51-71`. Each has a
   `*_dir()` / `*_file()` resolver method at `:78-105` that does
   `.clone().unwrap_or_else(|| PathBuf::from(DEFAULT_…))`. Same
   trick four times.

## What to do

1. Change `allowed_origins` to `Vec<String>` with `#[serde(default)]`.
   Empty vec is the unset signal. Simplify
   `validate_admin_config`'s match accordingly.
2. Change `session_lifetime_secs` to `u64` with
   `#[serde(default = "default_session_lifetime_secs")]` where
   the default fn returns `DEFAULT_SESSION_LIFETIME_SECS`.
3. Change the four `Option<PathBuf>` fields to `PathBuf` each with
   `#[serde(default = "default_<thing>_dir")]`. Default fns return
   `PathBuf::from(DEFAULT_<THING>)` matching today's constants.
4. Delete the four `*_dir()` / `*_file()` resolver methods. Callers
   read the fields directly.
5. Verify `examples/config.yaml` and the NixOS module
   (`nix/modules/underwrap-backend.nix`) still produce config that
   round-trips through the new schema (each defaulted field can be
   omitted, but should also accept an explicit value).

## Acceptance

- `cargo test --workspace`
- `cargo test -p integration-tests`
- `nix build .#checks.x86_64-linux.vm-test` (verifies the NixOS
  module's emitted YAML still deserializes correctly)
- `cargo clippy --all-targets --workspace -- -D warnings`
