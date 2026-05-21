//! Drift detection for the TypeScript bindings under `web/src/api/types/`.
//!
//! Each admin-API DTO derives `ts_rs::TS` with `#[ts(export)]`, which
//! auto-emits a per-type `export_bindings_<name>` test that *writes* the
//! corresponding `.ts` file. Those tests don't catch drift on their own:
//! they're happy to overwrite stale files silently.
//!
//! This test runs after (or alongside) those auto-tests and:
//!
//! 1. Re-computes the TypeScript declaration for every registered DTO
//!    via `T::export_to_string` (the same code path the auto-tests use).
//! 2. Reads the file on disk under `web/src/api/types/<Name>.ts`.
//! 3. Compares — if any differ, the test fails with a list of the
//!    offending types and instructions to regenerate.
//! 4. Asserts that the barrel `web/src/api/types/index.ts` re-exports
//!    exactly the registered set, so the SPA can `import type { … }
//!    from "@/api/types"` without each file path.
//!
//! Adding a new admin DTO means: derive `TS`, then add the type to the
//! `register!` list below. Forgetting to add it makes this test fail
//! with a clear message rather than silently shipping an untracked file.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ts_rs::TS;

use underwrap_backendd::{
    BootstrapRequest, BootstrapResponse, BootstrapStatusResponse, LoginRequest, LoginResponse,
    WhoamiResponse,
};
use underwrap_protocol::entities::{
    EntityCreator, PairsDocument, ResourceRecord, ResourcesDocument, WrapperResourcePairRecord,
};
use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
use underwrap_protocol::resource::ResourceSlot;
use underwrap_protocol::service::{
    AdminClientInstanceActionState, AdminClientInstanceCreateRequest,
    AdminClientInstanceDetailResponse, AdminClientInstanceListItem, AdminClientInstanceStatus,
    AdminClientInstanceUpdateRequest, AdminClientInstancesListResponse, AdminOkUserResponse,
    AdminPasswordChangeRequest, AdminStoreActionState, AdminStoreBackendKind,
    AdminStoreCreateRequest, AdminStoreCreateResponse, AdminStoreDeleteResponse,
    AdminStoreDetailResponse, AdminStoreListItem, AdminStoreRotateKeyResponse,
    AdminStoreUnlockRequest, AdminStoreUpdateRequest, AdminStoreUsageSummary,
    AdminStoresListResponse, AdminUserCreateRequest, AdminUserDetailResponse, AdminUserListItem,
    AdminUserResetPasswordRequest, AdminUserUpdateRequest, AdminUsersListResponse,
};
use underwrap_protocol::service::{
    AdminOkResponse, AdminPairCreateRequest, AdminPairCreateResponse, AdminPairDeleteBlockers,
    AdminPairDeleteRequest, AdminPairDeleteResponse, AdminPairsListRequest, AdminPairsListResponse,
    AdminPolicyGetRequest, AdminPolicyGetResponse, AdminPolicyPutRequest,
    AdminResourceBlobGetRequest, AdminResourceBlobGetResponse, AdminResourceBlobPutRequest,
    AdminResourceBlobPutResponse, AdminResourceCreateRequest, AdminResourceCreateResponse,
    AdminResourceDeleteBlockers, AdminResourceDeleteRequest, AdminResourceDeleteResponse,
    AdminResourceRenameRequest, AdminResourceRenameResponse, AdminResourcesListRequest,
    AdminResourcesListResponse, AdminSecretDeleteRequest, AdminSecretDeleteResponse,
    AdminSecretEntry, AdminSecretListRequest, AdminSecretListResponse, AdminSecretReadRequest,
    AdminSecretReadResponse, AdminSecretRenameRequest, AdminSecretWriteRequest,
    AdminUiOverviewResponse, AdminUiOverviewStore, AdminUiWrapperCounts,
    AdminWrapperPermissionDeleteRequest, AdminWrapperPermissionKey,
    AdminWrapperPermissionUpsertRequest, AdminWrapperPermissionsListRequest,
    AdminWrapperPermissionsListResponse, ServiceWrapperResourcePair,
};

fn export_dir() -> PathBuf {
    ts_rs::Config::from_env().out_dir().to_path_buf()
}

fn check<T: TS + 'static>(out: &mut Vec<String>, names: &mut BTreeSet<String>) {
    let cfg = ts_rs::Config::from_env();
    let rel = T::output_path().expect("TS-derived types always have an output path");
    let path = export_dir().join(&rel);
    let expected = T::export_to_string(&cfg).expect("export_to_string");
    let actual = std::fs::read_to_string(&path).unwrap_or_default();
    if expected != actual {
        out.push(format!(
            "  {}  ({})",
            std::any::type_name::<T>(),
            path.display()
        ));
    }
    // The TS type name is the file stem (ts-rs filename matches the TS
    // export name 1:1 because we don't use `export_to`).
    let stem = rel
        .file_stem()
        .and_then(|s| s.to_str())
        .expect("output path has a UTF-8 stem")
        .to_string();
    names.insert(stem);
}

macro_rules! check_all {
    ($out:ident, $names:ident, $($t:ty),* $(,)?) => {
        $( check::<$t>(&mut $out, &mut $names); )*
    };
}

#[test]
fn ts_bindings_in_sync_with_rust() {
    let mut drift: Vec<String> = Vec::new();
    let mut names: BTreeSet<String> = BTreeSet::new();

    check_all!(
        drift,
        names,
        // backend-backendd auth DTOs
        BootstrapRequest,
        BootstrapResponse,
        BootstrapStatusResponse,
        LoginRequest,
        LoginResponse,
        WhoamiResponse,
        // shared-protocol::policy types reachable from admin DTOs
        WrapperPermission,
        PolicyDocument,
        WrapperApprovalMode,
        // shared-protocol::entities — resource/pair entity model
        EntityCreator,
        ResourceRecord,
        ResourcesDocument,
        WrapperResourcePairRecord,
        PairsDocument,
        // shared-protocol::service admin DTOs
        AdminWrapperPermissionsListRequest,
        AdminWrapperPermissionsListResponse,
        AdminWrapperPermissionUpsertRequest,
        AdminWrapperPermissionDeleteRequest,
        AdminOkResponse,
        AdminPolicyGetRequest,
        AdminPolicyGetResponse,
        AdminPolicyPutRequest,
        AdminSecretDeleteRequest,
        AdminSecretDeleteResponse,
        AdminSecretEntry,
        AdminSecretListRequest,
        AdminSecretListResponse,
        AdminSecretReadRequest,
        AdminSecretReadResponse,
        AdminSecretWriteRequest,
        ResourceSlot,
        AdminStoreBackendKind,
        AdminStoreUsageSummary,
        AdminStoreActionState,
        AdminStoreListItem,
        AdminStoresListResponse,
        AdminStoreDetailResponse,
        AdminStoreCreateRequest,
        AdminStoreCreateResponse,
        AdminStoreUpdateRequest,
        AdminStoreDeleteResponse,
        AdminStoreRotateKeyResponse,
        AdminStoreUnlockRequest,
        AdminClientInstanceStatus,
        AdminClientInstanceActionState,
        AdminClientInstanceListItem,
        AdminClientInstancesListResponse,
        AdminClientInstanceDetailResponse,
        AdminClientInstanceCreateRequest,
        AdminClientInstanceUpdateRequest,
        AdminUserListItem,
        AdminUsersListResponse,
        AdminUserDetailResponse,
        AdminUserCreateRequest,
        AdminUserUpdateRequest,
        AdminUserResetPasswordRequest,
        AdminPasswordChangeRequest,
        AdminOkUserResponse,
        AdminUiOverviewResponse,
        AdminUiOverviewStore,
        AdminUiWrapperCounts,
        // Admin Resource/Pair CRUD (ticket 384)
        AdminWrapperPermissionKey,
        AdminResourcesListRequest,
        AdminResourcesListResponse,
        AdminResourceCreateRequest,
        AdminResourceCreateResponse,
        AdminResourceRenameRequest,
        AdminResourceRenameResponse,
        AdminSecretRenameRequest,
        AdminResourceDeleteRequest,
        AdminResourceDeleteBlockers,
        AdminResourceDeleteResponse,
        AdminPairsListRequest,
        AdminPairsListResponse,
        AdminPairCreateRequest,
        AdminPairCreateResponse,
        AdminPairDeleteRequest,
        AdminPairDeleteBlockers,
        AdminPairDeleteResponse,
        ServiceWrapperResourcePair,
        // Admin resource blob round-trip (ticket 404)
        AdminResourceBlobGetRequest,
        AdminResourceBlobGetResponse,
        AdminResourceBlobPutRequest,
        AdminResourceBlobPutResponse,
    );

    if !drift.is_empty() {
        panic!(
            "TypeScript bindings drift from Rust definitions:\n{}\n\n\
             Run `cargo test --workspace` to regenerate `web/src/api/types/`, \
             then commit the updated files.",
            drift.join("\n")
        );
    }

    // 2) Surface bindings that exist on disk but aren't registered above.
    //    Without this, a dev could add `#[ts(export)]` to a type, the
    //    auto-test writes the file, and we'd ship it un-tracked.
    let dir = export_dir();
    let mut on_disk: BTreeSet<String> = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).expect("read web/src/api/types/") {
        let entry = entry.expect("dir entry");
        let p = entry.path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name == "index.ts" {
            continue;
        }
        if let Some(stem) = name.strip_suffix(".ts") {
            on_disk.insert(stem.to_string());
        }
    }
    let extra: Vec<&String> = on_disk.difference(&names).collect();
    let missing: Vec<&String> = names.difference(&on_disk).collect();
    if !extra.is_empty() || !missing.is_empty() {
        panic!(
            "TypeScript bindings registry out of sync with `web/src/api/types/`:\n  \
             missing on disk (registered but no file): {:?}\n  \
             extra on disk   (file but not registered): {:?}\n\n\
             If you added a new admin DTO, also add it to the `check_all!` \
             list in this file. If you removed one, delete its `.ts` file too.",
            missing, extra,
        );
    }

    // 3) The barrel `index.ts` must re-export every registered type so the
    //    SPA can write `import type { LoginRequest } from "@/api/types"`.
    let mut expected_index = String::from(
        "// This file is auto-checked by `cargo test --test ts_export`. \
         Regenerate by running\n// `cargo test --workspace` after changing a \
         Rust admin DTO; the test below will\n// fail until the contents \
         match the registered type list.\n\n",
    );
    for name in &names {
        expected_index.push_str(&format!("export type {{ {name} }} from \"./{name}\";\n"));
    }
    let index_path = dir.join("index.ts");
    let actual_index = std::fs::read_to_string(&index_path).unwrap_or_default();
    if actual_index != expected_index {
        // Self-heal so the dev only has to commit, not write the file by hand.
        std::fs::write(&index_path, &expected_index).expect("write index.ts");
        panic!(
            "`{}` was out of sync with the registered types; it has been \
             rewritten. Commit the updated file.",
            index_path.display()
        );
    }
}
