use super::*;

#[tokio::test]
async fn pair_last_used_is_bumped_on_successful_wrapper_read() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    // Seed the resource and pair via admin so a pair record exists
    // for the backend to bump.
    let resource_record = daemon
        .create_resource("test", "github/token")
        .await
        .unwrap();
    let resource_id = resource_record.resource.id.clone();
    daemon
        .create_pair("test", "gh-authorized", &resource_id)
        .await
        .unwrap();

    // Before the wrapper invocation, last_used_at is None.
    let before = daemon.list_pairs("test").await.unwrap();
    let before_pair = before
        .pairs
        .iter()
        .find(|p| p.wrapper == "gh-authorized" && p.resource_id == resource_id)
        .expect("pair seeded above");
    assert!(
        before_pair.last_used_at.is_none(),
        "expected no last_used_at on fresh pair, got {:?}",
        before_pair.last_used_at,
    );

    // A successful wrapper read should bump the timestamp.
    daemon
        .store_secret("test", "gh-authorized", &resource_id, "ghp_test123")
        .await
        .unwrap();
    let (_, blob, _) = checkout_resource(daemon.instance_socket.clone(), "gh-authorized").await;
    assert_eq!(expect_single_env_string(blob), "ghp_test123");

    let after = daemon.list_pairs("test").await.unwrap();
    let after_pair = after
        .pairs
        .iter()
        .find(|p| p.wrapper == "gh-authorized" && p.resource_id == resource_id)
        .expect("pair still present");
    assert!(
        after_pair.last_used_at.is_some(),
        "expected last_used_at to be set after a successful read",
    );

    daemon.stop().await.unwrap();
}
