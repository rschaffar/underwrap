# Story: Seam — naming and wire-format cleanup across SPA ↔ backend

Part of: `epic_web_backend_cleanup_2026-05`.

## Background

Six cross-component contract changes surfaced by the 2026-05-24 review
(`tickets/_review/web_backend_seam_2026-05-24.md`). They touch both the
backend admin API and the SPA, so sequence them first — the per-side
refactors that follow consume the new shapes.

## Children

- `1010_task_rename_storeName_to_storeId.md` — SPA-only rename (S-1)
- `1020_task_lock_response_to_shared_protocol.md` — DTO relocation (S-2)
- `1030_task_logout_204_and_drop_emptyobject.md` — fix wire/doc drift (S-3)
- `1040_task_narrow_secret_list_slot_schema.md` — OpenAPI schema tighten (S-4)
- `1050_task_structured_store_delete_cleanup_hint.md` — restructure response (S-5)
- `1060_task_atomic_pair_with_new_resource.md` — new endpoint, removes
  orphan-resource failure mode (S-6, S-9)

The `cookies_secure` plumbing (S-10) lives in `1220_task_split_admin_runtime_state`
since it's an internal wiring fix tied to that refactor.

S-7 (duplicate policy-viewer query) and S-8 (lazy libsecret tree) land
in `1330_task_lazy_libsecret_tree` — they're SPA-only perf/clarity wins.

## Acceptance

After all six children: full backend + SPA test suites green, and the
OpenAPI drift test (`cargo test --test openapi_export`) reflects the
new wire shapes.
