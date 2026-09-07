use super::*;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[test]
fn backend_thread_owner_notifies_and_joins_on_drop() {
    let shutdown = Arc::new(Notify::new());
    let observed_shutdown = Arc::new(AtomicBool::new(false));
    let thread_shutdown = Arc::clone(&shutdown);
    let thread_observed = Arc::clone(&observed_shutdown);
    let handle = thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(thread_shutdown.notified());
        thread_observed.store(true, Ordering::SeqCst);
        Ok(())
    });

    let owner = BackendThreadOwner::new(shutdown, handle);
    drop(owner);

    assert!(observed_shutdown.load(Ordering::SeqCst));
}

#[tokio::test]
async fn daemon_task_owner_aborts_on_drop() {
    struct DropSignal(Arc<AtomicBool>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let started = Arc::new(Notify::new());
    let task_started = Arc::clone(&started);
    let dropped = Arc::new(AtomicBool::new(false));
    let task_dropped = Arc::clone(&dropped);
    let handle = tokio::spawn(async move {
        let _drop_signal = DropSignal(task_dropped);
        task_started.notify_one();
        std::future::pending::<Result<()>>().await
    });
    started.notified().await;

    let owner = DaemonTaskOwner::new(handle);
    drop(owner);
    tokio::task::yield_now().await;

    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_daemon_drop_eventually_removes_tempdir_after_owned_services_stop() {
    let wrappers = HashMap::from([(
        "example".to_string(),
        wrapper("/bin/true", "resource", "TOKEN"),
    )]);
    let daemon = TestDaemon::start(wrappers).await.unwrap();
    let tempdir = daemon._tempdir.path().to_path_buf();
    assert!(tempdir.exists());

    drop(daemon);

    tokio::time::timeout(Duration::from_secs(2), async {
        while tempdir.exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("fixture temporary directory remained after fallback cleanup");
}
