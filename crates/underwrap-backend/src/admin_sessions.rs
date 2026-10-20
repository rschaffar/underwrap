//! In-memory admin sessions with update-only renewal.
//!
//! The inner MemoryStore is private: all clones share the same operation gate.
//! Checking liveness and saving under that gate prevents a late response from
//! recreating a record after logout/delete or after its inactivity deadline.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::Mutex;
use tower_sessions::session::{Id, Record};
use tower_sessions::{MemoryStore, SessionStore, session_store};

#[derive(Clone, Default)]
pub(crate) struct AdminSessionStore(Arc<Mutex<MemoryStore>>);

impl fmt::Debug for AdminSessionStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdminSessionStore").finish_non_exhaustive()
    }
}

type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = session_store::Result<T>> + Send + 'a>>;

// SessionStore uses async-trait's boxed ABI. Spell it out here rather than
// adding a dependency solely for the macro; these are the same borrow bounds.
impl SessionStore for AdminSessionStore {
    fn create<'a, 'b, 's>(&'a self, record: &'b mut Record) -> StoreFuture<'s, ()>
    where
        'a: 's,
        'b: 's,
        Self: 's,
    {
        Box::pin(async move { self.0.lock().await.create(record).await })
    }

    fn save<'a, 'b, 's>(&'a self, record: &'b Record) -> StoreFuture<'s, ()>
    where
        'a: 's,
        'b: 's,
        Self: 's,
    {
        Box::pin(async move {
            let store = self.0.lock().await;
            if store.load(&record.id).await?.is_some() {
                store.save(record).await?;
            }
            // A revoked/expired record is deliberately not recreated. The
            // admitted request may still succeed and emit a stale cookie, but
            // replay cannot recover authority. Only create admits new IDs.
            Ok(())
        })
    }

    fn load<'a, 'b, 's>(&'a self, id: &'b Id) -> StoreFuture<'s, Option<Record>>
    where
        'a: 's,
        'b: 's,
        Self: 's,
    {
        Box::pin(async move { self.0.lock().await.load(id).await })
    }

    fn delete<'a, 'b, 's>(&'a self, id: &'b Id) -> StoreFuture<'s, ()>
    where
        'a: 's,
        'b: 's,
        Self: 's,
    {
        Box::pin(async move { self.0.lock().await.delete(id).await })
    }
}

#[cfg(test)]
mod tests;
