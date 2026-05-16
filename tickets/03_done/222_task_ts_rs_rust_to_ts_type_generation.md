# Task: ts-rs Rust → TypeScript type generation

Parent: `220_story_vite_react_shadcn_spa_scaffold.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Add the `ts-rs` crate dependency to `crates/shared-protocol` and `crates/backend-backendd` for every DTO that crosses the admin API boundary (request bodies, response shapes, enums).
- `#[derive(TS)] #[ts(export, export_to = "../../web/src/api/types/")]` per DTO, or factor exports into a single workspace test that triggers full export.
- Generated `web/src/api/types.ts` (re-exporting from the per-DTO files) is committed so the SPA build does not depend on the Rust toolchain at SPA build time.
- A `cargo test --test ts_export` (or `cargo test -p shared-protocol --test types`) runs the export and fails if the on-disk output differs from the committed files.
- CI runs that test. Local convenience: `make types` or a `cargo` alias.

## Acceptance criteria

- [ ] All admin API DTOs derive `TS`.
- [ ] `web/src/api/types.ts` exists and is committed.
- [ ] Drift between Rust types and committed TS fails `cargo test`.
- [ ] SPA imports types from `@/api/types` and `npm run build` typechecks.
- [ ] Adding a field to a DTO triggers a clear test failure pointing to the file to regenerate.
- [ ] No types regress to `any`/`unknown` in TS output (verified by a tsc check that fails on implicit `any`).
