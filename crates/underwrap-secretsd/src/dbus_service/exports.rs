//! Metadata-only export inventory; never clone secret material for reconciliation.
use std::{collections::BTreeSet, sync::Arc};

use tokio::sync::RwLock;
use underwrap_secretsd_store::SecretStore;
use zbus::{ObjectServer, fdo};

use super::{CollectionInterface, ItemInterface, alias_path, collection_path, item_path};
use crate::publication::SharedPublisher;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Export {
    Collection(String),
    Alias(String),
    Item(String, String),
}

#[derive(Default)]
pub(super) struct Exports(BTreeSet<Export>);

impl Exports {
    pub(super) fn from_store(store: &SecretStore) -> Self {
        let mut exports = BTreeSet::new();
        for (id, collection) in &store.collections {
            exports.insert(Export::Collection(id.clone()));
            for item in collection.items.keys() {
                exports.insert(Export::Item(id.clone(), item.clone()));
            }
        }
        exports.extend(store.aliases.keys().cloned().map(Export::Alias));
        Self(exports)
    }

    pub(super) async fn reconcile(
        &self,
        next: &Self,
        object_server: &ObjectServer,
        store: &Arc<RwLock<SecretStore>>,
        publisher: &SharedPublisher,
    ) -> fdo::Result<()> {
        // Graph methods are serialized, while content setters can finish under
        // registry reads. Never hold the store lock while acquiring the registry.
        // Old exports re-resolve accepted state; removal cannot revoke retained refs.
        // Unregister descendants first; zbus prunes nodes and empty ancestors
        // only after their remaining interfaces and children are gone.
        for export in self
            .0
            .iter()
            .rev()
            .filter(|export| !next.0.contains(export))
        {
            let removed = match export {
                Export::Collection(id) => {
                    object_server
                        .remove::<CollectionInterface, _>(collection_path(id)?)
                        .await
                }
                Export::Alias(name) => {
                    object_server
                        .remove::<CollectionInterface, _>(alias_path(name)?)
                        .await
                }
                Export::Item(collection, id) => {
                    object_server
                        .remove::<ItemInterface, _>(
                            item_path(collection, id).ok_or_else(export_error)?,
                        )
                        .await
                }
            };
            // remove's bool describes whether the node was destroyed, not whether
            // the requested interface was removed.
            removed.map_err(|_| export_error())?;
        }
        // at returns the supplied interface on collision; only None establishes
        // a new registration, preserving collision-as-failure semantics.
        for export in next.0.difference(&self.0) {
            let registered = match export {
                Export::Collection(id) => object_server
                    .at(
                        collection_path(id)?,
                        CollectionInterface::new(id.clone(), store.clone(), publisher.clone()),
                    )
                    .await
                    .map(|existing| existing.is_none()),
                Export::Alias(name) => object_server
                    .at(
                        alias_path(name)?,
                        CollectionInterface::alias(name.clone(), store.clone(), publisher.clone()),
                    )
                    .await
                    .map(|existing| existing.is_none()),
                Export::Item(collection, id) => object_server
                    .at(
                        item_path(collection, id).ok_or_else(export_error)?,
                        ItemInterface::new(
                            id.clone(),
                            collection.clone(),
                            store.clone(),
                            publisher.clone(),
                        ),
                    )
                    .await
                    .map(|existing| existing.is_none()),
            }
            .map_err(|_| export_error())?;
            if !registered {
                return Err(export_error());
            }
        }
        Ok(())
    }
}

fn export_error() -> fdo::Error {
    fdo::Error::Failed("Secret Service export reconciliation failed".into())
}

/// Armed immediately after ACK/live commit, before any async registry work.
/// Cancellation or unwinding must not release the graph-method gate and resume
/// service with a partially reconciled graph either.
pub(super) struct ReconcileGuard(bool);

impl ReconcileGuard {
    pub(super) fn new() -> Self {
        Self(true)
    }

    pub(super) fn finish(mut self, result: fdo::Result<()>) {
        if result.is_ok() {
            self.0 = false;
        }
    }
}

impl Drop for ReconcileGuard {
    fn drop(&mut self) {
        if self.0 {
            // A host-ACKed generation cannot be rolled back. Never serve a
            // partially reconciled graph or report an ordinary rejected mutation.
            eprintln!("underwrap-secretsd: accepted state export reconciliation failed; stopping");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests;
