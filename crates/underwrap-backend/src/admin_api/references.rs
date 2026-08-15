//! Selected-store entity-reference validation for admin mutations.
//!
//! These checks deliberately remain separate reads from the operation they
//! guard. They provide deterministic integrity for serialized admin requests;
//! they are not transactions or compare-and-swap protection.

use underwrap_backend_store::StorageBackend;

use crate::admin_api::AdminError;
use crate::admin_api::util::IntoInternal;
use crate::runtime_state::ListenerState;
use crate::storage::documents::{load_pairs_document, load_resources_document};

/// Require an exact catalog resource in the selected store.
pub(super) fn require_resource(
    store: &dyn StorageBackend,
    resource_id: &str,
) -> Result<(), AdminError> {
    let resources = load_resources_document(store).into_internal("loading resources document")?;
    resources
        .find_by_id(resource_id)
        .map(|_| ())
        .ok_or_else(AdminError::resource_not_found)
}

/// Require an exact runtime client in the published registry and assigned to
/// the selected store. Disabled clients remain valid references.
pub(super) fn require_client_instance_in_store(
    state: &ListenerState,
    store_id: &str,
    client_instance_id: &str,
) -> Result<(), AdminError> {
    state
        .shared
        .client_instances
        .read()
        .unwrap()
        .get(client_instance_id)
        .filter(|record| record.current_store_id == store_id)
        .map(|_| ())
        .ok_or_else(AdminError::client_instance_not_found)
}

/// Require an exact `(wrapper, resource_id)` pair in the selected store.
pub(super) fn require_pair(
    store: &dyn StorageBackend,
    wrapper: &str,
    resource_id: &str,
) -> Result<(), AdminError> {
    let pairs = load_pairs_document(store).into_internal("loading pairs document")?;
    pairs
        .find(wrapper, resource_id)
        .map(|_| ())
        .ok_or_else(AdminError::pair_not_found)
}

#[cfg(test)]
mod tests;
