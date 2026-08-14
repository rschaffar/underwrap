use std::mem;

use underwrap_secretsd_store::SecretStore;
use zbus::fdo;
use zeroize::Zeroize;

use crate::publication::{SharedPublisher, publish_store};

/// Apply and publish one mutation without exposing unacknowledged state.
pub(super) fn mutate_and_publish<T>(
    live_store: &mut SecretStore,
    publisher: &SharedPublisher,
    mutate: impl FnOnce(&mut SecretStore) -> fdo::Result<T>,
) -> fdo::Result<T> {
    let mut candidate = CandidateStore::new(live_store.clone());
    let result = mutate(candidate.store_mut())?;
    publish_store(publisher, candidate.store())?;
    candidate.commit(live_store);
    Ok(result)
}

/// Owns a secret-bearing candidate until it is either committed or wiped.
struct CandidateStore {
    store: Option<SecretStore>,
}

impl CandidateStore {
    fn new(store: SecretStore) -> Self {
        Self { store: Some(store) }
    }

    fn store(&self) -> &SecretStore {
        self.store
            .as_ref()
            .expect("candidate store remains present before commit")
    }

    fn store_mut(&mut self) -> &mut SecretStore {
        self.store
            .as_mut()
            .expect("candidate store remains present before commit")
    }

    fn commit(mut self, live_store: &mut SecretStore) {
        let candidate = self
            .store
            .take()
            .expect("candidate store remains present at commit");
        let mut displaced = mem::replace(live_store, candidate);
        zeroize_item_secrets(&mut displaced);
    }
}

impl Drop for CandidateStore {
    fn drop(&mut self) {
        if let Some(store) = &mut self.store {
            zeroize_item_secrets(store);
        }
    }
}

fn zeroize_item_secrets(store: &mut SecretStore) {
    for collection in store.collections.values_mut() {
        for item in collection.items.values_mut() {
            item.secret.zeroize();
        }
    }
}

#[cfg(test)]
mod tests;
