//! `GET /v1/admin/ui/overview` — the SPA's dashboard summary tile.
//!
//! Aggregates per-store resource count + wrapper-permission mode
//! buckets. Locked encrypted stores are silently skipped so an
//! operator who can't unlock them isn't bombarded with `store_locked`
//! from a tile they can't act on directly.

use anyhow::Result;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};
use std::sync::Arc;

use underwrap_store::StorageBackend;

use crate::http::responses::{error_response, json_response};
use crate::runtime_state::AdminRuntimeState;
use crate::storage::documents::{load_policy_document, load_resources_document};

pub(crate) fn admin_ui_overview_for_store(
    store_name: &str,
    store: &dyn StorageBackend,
) -> Result<underwrap_protocol::service::AdminUiOverviewStore> {
    let resources = load_resources_document(store)?;
    let policy = load_policy_document(store)?;

    let resource_count = resources.resources.len();

    // Bucket every wrapper permission by its current mode. The
    // permission key is `(client_instance, wrapper, resource)` so this
    // count reflects the distribution across the per-resource
    // permission fleet, not the wrapper catalog.
    let mut wrapper_counts = underwrap_protocol::service::AdminUiWrapperCounts {
        locked: 0,
        unlocked: 0,
        unlocked_until: 0,
    };
    let mut writable_permission_count = 0usize;
    for permission in &policy.permissions {
        match permission.mode {
            underwrap_protocol::policy::WrapperApprovalMode::Locked => wrapper_counts.locked += 1,
            underwrap_protocol::policy::WrapperApprovalMode::Unlocked => {
                wrapper_counts.unlocked += 1
            }
            underwrap_protocol::policy::WrapperApprovalMode::UnlockedUntil { .. } => {
                wrapper_counts.unlocked_until += 1
            }
        }
        if permission.writable {
            writable_permission_count += 1;
        }
    }

    Ok(underwrap_protocol::service::AdminUiOverviewStore {
        store: store_name.to_string(),
        resource_count,
        wrapper_counts,
        writable_permission_count,
    })
}

pub(crate) fn handle_admin_ui_overview(state: &AdminRuntimeState) -> Response<Full<Bytes>> {
    // Snapshot the unlocked backends so we can release the
    // read lock before doing I/O against each store. Locked
    // encrypted stores are silently skipped — the SPA's
    // overview already exposes their presence via the stores
    // list endpoint, and probing them here would surface
    // `store_locked` from a tile that the operator can't act
    // on directly.
    let snapshot: Vec<(String, Arc<dyn StorageBackend>)> = {
        let reg = state.stores.read().unwrap();
        reg.state
            .iter()
            .filter_map(|(name, state)| state.backend().map(|b| (name.clone(), Arc::clone(b))))
            .collect()
    };
    match snapshot
        .iter()
        .map(|(name, store)| admin_ui_overview_for_store(name, store.as_ref()))
        .collect::<Result<Vec<_>>>()
    {
        Ok(mut stores) => {
            stores.sort_by(|a, b| a.store.cmp(&b.store));
            json_response(
                StatusCode::OK,
                &underwrap_protocol::service::AdminUiOverviewResponse { stores },
            )
        }
        Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    }
}

pub(crate) fn router() -> Router<Arc<AdminRuntimeState>> {
    Router::new().route("/v1/admin/ui/overview", get(axum_overview))
}

async fn axum_overview(State(state): State<Arc<AdminRuntimeState>>) -> Response<Full<Bytes>> {
    handle_admin_ui_overview(&state)
}
