# Task: Extract `config`, `store_registry`, and `tls` modules

Parent: `700_story_backend_modularization.md`

## Scope

First mechanical step. Pull three self-contained pieces out of
`server.rs` into their own files. No callers outside the crate change;
`lib.rs` re-exports keep the public surface identical.

### `crates/backend-backendd/src/config.rs` (new)

Move verbatim from `server.rs`:

- `HttpsTlsConfig` (struct, currently lines 34-38)
- `HttpsConfig` (struct, 40-44)
- `AdminConfig` (struct, 46-67)
- `BackendConfig` (struct + `impl`, 72-116)
- `DEFAULT_STORES_FILE` const (70)

### `crates/backend-backendd/src/store_registry.rs` (new)

Move verbatim from `server.rs`:

- `LoadedStoreRecord` (121-123)
- `StoreState` + `impl` (131-147)
- `RegistryLookupError` (151-155)
- `StoreRegistry` + `impl` + `Default` (164-240)
- `open_initial_stores` (419-441)
- `load_store_records` (446-491)
- `save_store_records` (493-524)
- `is_valid_store_id` (529-541)
- `validate_store_name` (543-550)
- `generate_store_id` (552-560)

`open_initial_stores` and `save_store_records` are currently
`pub(crate)`-only — keep them at that visibility. `load_store_records`
and `is_valid_store_id` are currently `pub` (used by tests in
`integration-tests/`); keep them `pub` and re-export from `lib.rs`.

### `crates/backend-backendd/src/tls.rs` (new)

Move verbatim from `server.rs`:

- `build_https_acceptor` (595-612)
- `FingerprintClientCertVerifier` + `impl`s (624-681)
- `load_cert_chain` (749-762)
- `load_private_key` (764-782)

All `pub(crate)` — only `server.rs` calls these today.

### `crates/backend-backendd/src/lib.rs`

Add the three new module declarations and re-export the same public
names as today (`BackendConfig`, `HttpsConfig`, `HttpsTlsConfig`,
`AdminConfig`, `DEFAULT_STORES_FILE`, `load_store_records`,
`is_valid_store_id`). Update `server.rs` `use` lines accordingly.

## Acceptance criteria

- [ ] `config.rs`, `store_registry.rs`, `tls.rs` exist with the listed
      contents.
- [ ] `server.rs` no longer defines any of the listed items.
- [ ] No external crate has had to change its imports (the integration
      tests and `main.rs` still build with no edits beyond what this
      ticket already specifies in `lib.rs`).
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.

## Out of scope

- Renaming any of the moved items.
- Changing visibility of anything not explicitly listed.
- Moving tests. The `store_registry_tests` mod stays in `server.rs`
  for now; ticket 820 relocates it.
