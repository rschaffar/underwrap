# Epic: Web + backend cleanup pass — first-release polish

## Background

Bundles 18 task tickets emitted from the 2026-05-24 sweeping review
(`tickets/_review/web_backend_seam_2026-05-24.md`).

Scope: the admin SPA at `web/`, the `backend-backendd` crate, and the
OpenAPI seam between them. Posture is greenfield-aggressive — wire
renames, NixOS-module changes, and `pub` tightening are all in-bounds.
Backwards compatibility is not a concern.

The epic does not change any security boundary. Tickets that touch
auth/session/CSRF code (1100, 1150, 1210, 1220) carry the VM-test
acceptance check.

## Structure

```
1000_story_seam_naming_and_contract        — seam (cross-component contracts)
  1010_task_rename_storeName_to_storeId
  1020_task_lock_response_to_shared_protocol
  1030_task_logout_204_and_drop_emptyobject
  1040_task_narrow_secret_list_slot_schema
  1050_task_structured_store_delete_cleanup_hint
  1060_task_atomic_pair_with_new_resource

1100_task_runtime_api_into_response_unification    — admin/runtime
1110_task_admin_api_router_dedup
1120_task_admin_api_helpers_consolidation
1130_task_admin_api_mutate_collection_typed_errors
1140_task_admin_api_pub_tightening_and_dto_locations
1150_task_drop_client_instance_permissions_visibility

1200_task_collapse_json_dir_store                  — backend core
1210_task_collapse_loaded_store_record
1220_task_split_admin_runtime_state
1230_task_config_collapse_options
1240_task_lib_rs_pub_cleanup
1250_task_runtime_api_register_decomposition

1300_task_use_form_dialog_and_confirm_mutation     — web SPA
1310_task_collapse_api_client_wrappers
1320_task_router_guard_simplification
1330_task_lazy_libsecret_tree
1340_task_web_dialog_shell_unification
1350_task_web_misc_simplifications

1360_task_doc_comment_sweep                        — final pass
```

Numbering blocks:
- `1000–1099` seam (cross-component)
- `1100–1199` admin_api refactors
- `1200–1299` backend core refactors
- `1300–1399` web SPA refactors

## Recommended execution order

Seam first (1000 thread) so the per-side refactors consume the new
shapes. Then the two backend threads (1100, 1200) and the web thread
(1300) can run mostly in parallel. Finish with 1360 as a final
WHAT-comment + simplification sweep across all touched files.

## Acceptance

Every child ticket is closed with its own acceptance green, and a final
full-suite check passes:

- `cargo test --workspace`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cargo fmt -- --check`
- `cd web && npm test`
- `cd web && npm run build`
- `nix build .#checks.x86_64-linux.vm-test`
