use super::*;

#[tokio::test]
async fn backend_port_reservation_holds_the_process_startup_lock() {
    let reservation = reserve_backend_port().await.unwrap();

    assert_ne!(reservation.port(), 0);
    assert!(
        backend_port_startup_lock().try_lock().is_err(),
        "a live reservation must exclude another in-process backend startup"
    );
}
