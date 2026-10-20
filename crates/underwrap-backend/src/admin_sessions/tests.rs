use super::*;
use tower_sessions::cookie::time::{Duration, OffsetDateTime};

fn record(expiry_date: OffsetDateTime) -> Record {
    Record {
        id: Id::default(),
        data: Default::default(),
        expiry_date,
    }
}

#[tokio::test]
async fn only_create_admits_records_and_live_save_renews_them() {
    let store = AdminSessionStore::default();
    let mut candidate = record(OffsetDateTime::now_utc() + Duration::minutes(1));
    store.save(&candidate).await.unwrap();
    assert!(store.load(&candidate.id).await.unwrap().is_none());
    store.create(&mut candidate).await.unwrap();
    let original = store
        .load(&candidate.id)
        .await
        .unwrap()
        .unwrap()
        .expiry_date;
    candidate.expiry_date += Duration::minutes(1);
    store.save(&candidate).await.unwrap();
    assert!(
        store
            .load(&candidate.id)
            .await
            .unwrap()
            .unwrap()
            .expiry_date
            > original
    );

    // A clone is the same authority, not another unlocked view of MemoryStore.
    store.clone().delete(&candidate.id).await.unwrap();
    store.save(&candidate).await.unwrap();
    assert!(store.load(&candidate.id).await.unwrap().is_none());
}

#[tokio::test]
async fn save_cannot_revive_an_expired_record() {
    let store = AdminSessionStore::default();
    let mut candidate = record(OffsetDateTime::now_utc() - Duration::seconds(1));
    store.create(&mut candidate).await.unwrap();
    assert!(store.load(&candidate.id).await.unwrap().is_none());
    candidate.expiry_date = OffsetDateTime::now_utc() + Duration::minutes(1);
    store.save(&candidate).await.unwrap();
    assert!(store.load(&candidate.id).await.unwrap().is_none());
}
