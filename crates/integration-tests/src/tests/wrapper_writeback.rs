//! Cargo-level integration tests for the wrapper-resource channel
//! protocol: `Checkout` / `Checkin` / `UsageSignal`.
//!
//! These tests act as the wrapper-side of the IPC, driving the channel
//! against a real `underwrapd` connected to a real `underwrap-backend`.
//! They cover the daemon-observable surface of the writeback decision
//! tree. The wrapper-side journal/stderr UX (the "drop+warn" arms) and
//! the env-clear regression are exercised by the VM test (task 432).
//!
//! What each test verifies:
//!
//! - `clean_run_only_sends_usage_signal`: Checkout + UsageSignal with
//!   no Checkin keeps the backend blob byte-identical. This is the
//!   no-mutation path.
//! - `writable_grant_persists_checkin`: same Checkout but a mutated
//!   blob sent via Checkin lands in the backend.
//! - `readonly_grant_rejects_checkin`: the daemon refuses Checkin
//!   against a writable=false grant with `ResourceErrorCode::Unauthorized`.
//! - `grant_flipped_readonly_between_checkout_and_checkin`: a grant
//!   downgraded mid-run is observed at Checkin time, not Checkout.
//! - `noncanonical_checkin_rejected`: hand-crafted non-canonical bytes
//!   are rejected as `ResourceErrorCode::InvalidRequest`. This matches
//!   the admin PUT contract.

use std::collections::{BTreeMap, HashMap};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::Path;

use underwrap_protocol::ipc::{
    read_frame_sync, read_wrapper_resource_frame_sync, write_frame_sync,
    write_wrapper_resource_frame_sync,
};
use underwrap_protocol::message::{
    ResourceErrorCode, SecretBytes, WrapperRequest, WrapperResourceRequest,
    WrapperResourceResponse, WrapperResponse,
};
use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
use underwrap_protocol::resource_blob::ResourceBlob;
use underwrap_protocol::scm_rights;

use super::*;

/// Connect a sync wrapper-side `StdUnixStream`, send `Read`, verify
/// `ReadOk`, and receive the one channel fd via SCM_RIGHTS.
fn open_run(socket_path: &Path, wrapper_name: &str) -> StdUnixStream {
    let mut stream = StdUnixStream::connect(socket_path)
        .unwrap_or_else(|e| panic!("connect {}: {e}", socket_path.display()));
    write_frame_sync(
        &mut stream,
        &WrapperRequest::Read {
            wrapper_name: wrapper_name.to_string(),
            variant: underwrap_protocol::variant::WrapperVariant::Real,
            scope: None,
        },
    )
    .unwrap();
    let resp: WrapperResponse = read_frame_sync(&mut stream).unwrap();
    assert!(underwrap_protocol::ipc::encode_to_vec(&resp).unwrap().len() < 1024);
    let resource_id = match resp {
        WrapperResponse::ReadOk {
            resource_id,
            env_projection: true,
            ..
        } => resource_id,
        other => panic!("expected ReadOk, got {other:?}"),
    };

    let fd = scm_rights::recv_fd(stream.as_fd())
        .unwrap_or_else(|e| panic!("recv_fd for '{resource_id}': {e}"));
    stream.shutdown(std::net::Shutdown::Both).unwrap();
    drop(stream);
    StdUnixStream::from(fd)
}

fn send_checkout(stream: &mut StdUnixStream) -> (Vec<u8>, bool) {
    write_wrapper_resource_frame_sync(stream, &WrapperResourceRequest::Checkout).unwrap();
    let resp: WrapperResourceResponse = read_wrapper_resource_frame_sync(stream).unwrap();
    match resp {
        WrapperResourceResponse::CheckoutOk { blob, writable } => (blob, writable),
        other => panic!("expected CheckoutOk, got {other:?}"),
    }
}

fn send_checkin(stream: &mut StdUnixStream, blob: &[u8]) -> WrapperResourceResponse {
    write_wrapper_resource_frame_sync(
        stream,
        &WrapperResourceRequest::Checkin {
            blob: blob.to_vec(),
        },
    )
    .unwrap();
    read_wrapper_resource_frame_sync(stream).unwrap()
}

fn send_usage_signal(stream: &mut StdUnixStream) -> WrapperResourceResponse {
    write_wrapper_resource_frame_sync(stream, &WrapperResourceRequest::UsageSignal).unwrap();
    read_wrapper_resource_frame_sync(stream).unwrap()
}

fn writeback_wrapper_config() -> HashMap<String, underwrap_protocol::config::WrapperDef> {
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "gh-writeback".to_string(),
        wrapper("/usr/bin/gh", "github/token", "GH_TOKEN"),
    );
    wrappers
}

fn make_permission(
    daemon: &TestDaemon,
    wrapper: &str,
    resource: &str,
    writable: bool,
) -> WrapperPermission {
    WrapperPermission {
        client_instance: daemon.client_instance().to_string(),
        wrapper: wrapper.to_string(),
        resource_id: resource.to_string(),
        mode: WrapperApprovalMode::Unlocked,
        writable,
        reason: None,
    }
}

fn put_blob(daemon: &TestDaemon, resource: &str, blob: &ResourceBlob) -> Vec<u8> {
    let bytes = blob.to_canonical_bytes().unwrap();
    daemon.put_resource_blob("test", resource, blob).unwrap();
    bytes
}

fn get_blob(daemon: &TestDaemon, resource: &str) -> ResourceBlob {
    daemon.get_resource_blob("test", resource).unwrap()
}

fn blob_with_env(key: &str, value: &[u8]) -> ResourceBlob {
    let mut blob = ResourceBlob::empty();
    let mut env = BTreeMap::new();
    env.insert(key.to_string(), SecretBytes::from(value.to_vec()));
    blob.env = env;
    blob
}

/// Same shape as [`blob_with_env`] but writes to the `files` section
/// instead. The env section is admin-only on the new wire — any
/// runtime writeback that changes env is rejected with
/// `env-section-immutable` — so tests that need to verify a
/// successful Checkin under a writable grant must mutate a writable
/// slot. `files` carries through cleanly.
fn blob_with_files_entry(key: &str, value: &[u8]) -> ResourceBlob {
    let mut blob = ResourceBlob::empty();
    let mut files = BTreeMap::new();
    files.insert(key.to_string(), SecretBytes::from(value.to_vec()));
    blob.files = files;
    blob
}

fn close_channel(stream: StdUnixStream) {
    stream.shutdown(std::net::Shutdown::Both).unwrap();
    drop(stream);
}

// ===========================================================================
// Tests
// ===========================================================================

#[tokio::test]
async fn env_above_generic_frame_limit_checks_out_over_scoped_fd() {
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();
    let value = "x".repeat(70_000);
    daemon
        .store_secret("test", "gh-writeback", "github/token", &value)
        .await
        .unwrap();

    let socket = daemon.instance_socket.clone();
    tokio::task::spawn_blocking(move || {
        let mut channel = open_run(&socket, "gh-writeback");
        let (bytes, _) = send_checkout(&mut channel);
        let blob = ResourceBlob::from_canonical_bytes(&bytes).unwrap();
        assert_eq!(expect_single_env_bytes(blob).len(), 70_000);
        assert!(matches!(
            send_usage_signal(&mut channel),
            WrapperResourceResponse::UsageSignalOk
        ));
        close_channel(channel);
    })
    .await
    .unwrap();

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn exact_max_checkout_checkin_and_followup_checkout_round_trip() {
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();
    let initial = blob_with_exact_canonical_len(
        underwrap_protocol::resource_blob::MAX_CANONICAL_RESOURCE_BYTES,
    );
    let initial_bytes = put_blob(&daemon, "github/token", &initial);
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                permissions: vec![make_permission(
                    &daemon,
                    "gh-writeback",
                    "github/token",
                    true,
                )],
            },
        )
        .await
        .unwrap();

    let mut mutated = initial.clone();
    let (key, value) = mutated.files.pop_first().unwrap();
    let mut changed_key = key.into_bytes();
    *changed_key.last_mut().unwrap() = b'y';
    mutated
        .files
        .insert(String::from_utf8(changed_key).unwrap(), value);
    let mutated_bytes = mutated.to_canonical_bytes().unwrap();
    assert_eq!(mutated_bytes.len(), initial_bytes.len());

    let socket = daemon.instance_socket.clone();
    let expected = mutated_bytes.clone();
    tokio::task::spawn_blocking(move || {
        let mut first = open_run(&socket, "gh-writeback");
        assert_eq!(send_checkout(&mut first).0, initial_bytes);
        assert!(matches!(
            send_checkin(&mut first, &expected),
            WrapperResourceResponse::CheckinOk
        ));
        close_channel(first);

        let mut second = open_run(&socket, "gh-writeback");
        let (checked_out, _) = send_checkout(&mut second);
        assert_eq!(
            checked_out.len(),
            underwrap_protocol::resource_blob::MAX_CANONICAL_RESOURCE_BYTES
        );
        assert_eq!(checked_out, expected);
        assert!(matches!(
            send_usage_signal(&mut second),
            WrapperResourceResponse::UsageSignalOk
        ));
        close_channel(second);
    })
    .await
    .unwrap();
    assert_eq!(get_blob(&daemon, "github/token"), mutated);
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn one_byte_over_max_is_not_written_and_prior_state_remains() {
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();
    let initial = blob_with_files_entry("state", b"prior");
    put_blob(&daemon, "github/token", &initial);
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                permissions: vec![make_permission(
                    &daemon,
                    "gh-writeback",
                    "github/token",
                    true,
                )],
            },
        )
        .await
        .unwrap();
    let oversized = canonical_one_byte_over_limit();
    let socket = daemon.instance_socket.clone();
    tokio::task::spawn_blocking(move || {
        let mut channel = open_run(&socket, "gh-writeback");
        let _ = send_checkout(&mut channel);
        let error = write_wrapper_resource_frame_sync(
            &mut channel,
            &WrapperResourceRequest::Checkin { blob: oversized },
        )
        .unwrap_err();
        assert!(error.to_string().contains("resource blob exceeds"));
        assert!(matches!(
            send_usage_signal(&mut channel),
            WrapperResourceResponse::UsageSignalOk
        ));
        close_channel(channel);
    })
    .await
    .unwrap();
    assert_eq!(get_blob(&daemon, "github/token"), initial);
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn clean_run_only_sends_usage_signal() {
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();

    let initial = blob_with_env("GH_TOKEN", b"ghp_initial");
    let initial_bytes = put_blob(&daemon, "github/token", &initial);
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                permissions: vec![make_permission(
                    &daemon,
                    "gh-writeback",
                    "github/token",
                    true,
                )],
            },
        )
        .await
        .unwrap();

    let socket = daemon.instance_socket.clone();
    // All sync channel I/O happens inside spawn_blocking so it
    // doesn't block the current-thread tokio runtime the daemon
    // tasks are running on.
    tokio::task::spawn_blocking(move || {
        let mut channel = open_run(&socket, "gh-writeback");
        let (blob, writable) = send_checkout(&mut channel);
        assert_eq!(blob, initial_bytes);
        assert!(writable);

        let resp = send_usage_signal(&mut channel);
        assert!(matches!(resp, WrapperResourceResponse::UsageSignalOk));
        close_channel(channel);
    })
    .await
    .unwrap();

    // Backend blob is byte-identical to the initial state.
    let after = get_blob(&daemon, "github/token");
    assert_eq!(after, initial);

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn writable_grant_persists_checkin() {
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();

    let initial = blob_with_files_entry(".config/gh/hosts.yml", b"github.com:\n  user: alice\n");
    put_blob(&daemon, "github/token", &initial);
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                permissions: vec![make_permission(
                    &daemon,
                    "gh-writeback",
                    "github/token",
                    true,
                )],
            },
        )
        .await
        .unwrap();

    let mutated = blob_with_files_entry(".config/gh/hosts.yml", b"github.com:\n  user: bob\n");
    let mutated_bytes = mutated.to_canonical_bytes().unwrap();

    let socket = daemon.instance_socket.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut channel = open_run(&socket, "gh-writeback");
        let (blob, writable) = send_checkout(&mut channel);
        assert!(writable);
        assert!(blob != mutated_bytes);
        let resp = send_checkin(&mut channel, &mutated_bytes);
        assert!(matches!(resp, WrapperResourceResponse::CheckinOk));
        close_channel(channel);
    })
    .await;
    result.unwrap();

    let after = get_blob(&daemon, "github/token");
    assert_eq!(after, mutated);

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn readonly_grant_rejects_checkin() {
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();

    let initial = blob_with_env("GH_TOKEN", b"ghp_initial");
    put_blob(&daemon, "github/token", &initial);
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                // writable = false → daemon must refuse the Checkin.
                permissions: vec![make_permission(
                    &daemon,
                    "gh-writeback",
                    "github/token",
                    false,
                )],
            },
        )
        .await
        .unwrap();

    let mutated = blob_with_env("GH_TOKEN", b"ghp_attempted_rewrite");
    let mutated_bytes = mutated.to_canonical_bytes().unwrap();

    let socket = daemon.instance_socket.clone();
    tokio::task::spawn_blocking(move || {
        let mut channel = open_run(&socket, "gh-writeback");
        let (_blob, writable) = send_checkout(&mut channel);
        assert!(!writable, "grant is read-only at Checkout");

        let resp = send_checkin(&mut channel, &mutated_bytes);
        match resp {
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::Unauthorized,
                ..
            } => {}
            other => panic!("expected Unauthorized, got {other:?}"),
        }
        close_channel(channel);
    })
    .await
    .unwrap();

    // Backend blob unchanged.
    let after = get_blob(&daemon, "github/token");
    assert_eq!(after, initial);

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn grant_flipped_readonly_between_checkout_and_checkin() {
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();

    let initial = blob_with_env("GH_TOKEN", b"ghp_initial");
    put_blob(&daemon, "github/token", &initial);
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                permissions: vec![make_permission(
                    &daemon,
                    "gh-writeback",
                    "github/token",
                    true,
                )],
            },
        )
        .await
        .unwrap();

    // Orchestrate via std mpsc channels (the spawn_blocking thread is
    // outside the tokio runtime, so a sync wait is the cleanest fit).
    // The blocking thread signals "Checkout complete" before the test
    // thread flips the grant, so the Checkout reliably observes
    // writable=true and the Checkin reliably observes the flipped
    // grant.
    let socket = daemon.instance_socket.clone();
    let (checkout_done_tx, checkout_done_rx) = std::sync::mpsc::channel::<()>();
    let (flip_tx, flip_rx) = std::sync::mpsc::channel::<()>();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<WrapperResourceResponse>();
    let mutated = blob_with_env("GH_TOKEN", b"ghp_flipped");
    let mutated_bytes = mutated.to_canonical_bytes().unwrap();
    let blocking = tokio::task::spawn_blocking(move || {
        let mut channel = open_run(&socket, "gh-writeback");
        let (_blob, writable) = send_checkout(&mut channel);
        assert!(writable, "Checkout must observe the still-writable grant");
        checkout_done_tx.send(()).unwrap();
        // Block until the test thread has flipped the grant.
        flip_rx.recv().unwrap();
        let resp = send_checkin(&mut channel, &mutated_bytes);
        close_channel(channel);
        done_tx.send(resp).unwrap();
    });

    // Wait for the spawn_blocking to finish Checkout before flipping
    // the grant, otherwise the test thread can race the blocking
    // thread.
    tokio::task::spawn_blocking(move || checkout_done_rx.recv().unwrap())
        .await
        .unwrap();

    // Flip the grant to read-only via the admin API.
    daemon
        .upsert_permission(
            "test",
            &make_permission(&daemon, "gh-writeback", "github/token", false),
        )
        .await
        .unwrap();
    flip_tx.send(()).unwrap();
    blocking.await.unwrap();

    let resp = done_rx.recv().unwrap();
    match resp {
        WrapperResourceResponse::Error {
            code: ResourceErrorCode::Unauthorized,
            ..
        } => {}
        other => panic!("expected Unauthorized, got {other:?}"),
    }

    // Backend blob unchanged because the mid-run flip won.
    let after = get_blob(&daemon, "github/token");
    assert_eq!(after, initial);

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn noncanonical_checkin_rejected() {
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();

    let initial = blob_with_env("GH_TOKEN", b"ghp_initial");
    put_blob(&daemon, "github/token", &initial);
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                permissions: vec![make_permission(
                    &daemon,
                    "gh-writeback",
                    "github/token",
                    true,
                )],
            },
        )
        .await
        .unwrap();

    // Hand-crafted JSON with keys out of canonical order — the same
    // shape the admin PUT noncanonical-blob test uses.
    let noncanonical =
        br#"{"schema_version":1,"libsecret":{"aliases":{},"collections":{}},"files":{},"env":{}}"#
            .to_vec();

    let socket = daemon.instance_socket.clone();
    tokio::task::spawn_blocking(move || {
        let mut channel = open_run(&socket, "gh-writeback");
        let (_blob, _writable) = send_checkout(&mut channel);
        let resp = send_checkin(&mut channel, &noncanonical);
        match resp {
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::InvalidRequest,
                message,
            } => assert!(
                message.contains("noncanonical-blob") || message.contains("noncanonical_blob"),
                "expected noncanonical-blob marker, got: {message}",
            ),
            other => panic!("expected InvalidRequest, got {other:?}"),
        }
        close_channel(channel);
    })
    .await
    .unwrap();

    let after = get_blob(&daemon, "github/token");
    assert_eq!(after, initial);

    daemon.stop().await.unwrap();
}

// ===========================================================================
// last_used_at coverage
// ===========================================================================
//
// Every successful backend access for a `resources/<id>` path bumps the
// `(wrapper, resource)` pair's `last_used_at` to the current wall-clock
// second (see `bump_pair_last_used` in the backend). The wrapper-resource
// channel hits that path for all three verbs:
//
//   - Checkout    → backend READ  of `resources/<id>`
//   - Checkin     → backend WRITE of `resources/<id>` (writable grants)
//   - UsageSignal → backend READ  of `resources/<id>`
//
// These tests pin down that every verb's success-path bumps the pair,
// including the "rejected Checkin → drop+UsageSignal" path that the
// underwrap-ns decision tree synthesizes.

async fn find_pair_last_used(daemon: &TestDaemon, wrapper: &str, resource: &str) -> Option<u64> {
    let pairs = daemon.list_pairs("test").await.unwrap();
    pairs
        .pairs
        .into_iter()
        .find(|p| p.wrapper == wrapper && p.resource_id == resource)
        .expect("pair record present")
        .last_used_at
}

async fn seed_writable_permission(daemon: &TestDaemon, wrapper: &str, resource: &str) {
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                permissions: vec![make_permission(daemon, wrapper, resource, true)],
            },
        )
        .await
        .unwrap();
}

/// Create a resource (by `name`) and a pair binding `wrapper` to it.
/// Returns the resource id the backend assigned — every other helper
/// in this file consumes the id, not the name.
async fn seed_resource_and_pair(daemon: &TestDaemon, wrapper: &str, name: &str) -> String {
    let record = daemon.create_resource("test", name).await.unwrap();
    let id = record.resource.id;
    daemon.create_pair("test", wrapper, &id).await.unwrap();
    id
}

#[tokio::test]
async fn checkout_bumps_last_used() {
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();
    let resource_id = seed_resource_and_pair(&daemon, "gh-writeback", "github/token").await;
    seed_writable_permission(&daemon, "gh-writeback", &resource_id).await;

    assert!(
        find_pair_last_used(&daemon, "gh-writeback", &resource_id)
            .await
            .is_none()
    );

    let socket = daemon.instance_socket.clone();
    tokio::task::spawn_blocking(move || {
        let mut channel = open_run(&socket, "gh-writeback");
        let _ = send_checkout(&mut channel);
        close_channel(channel);
    })
    .await
    .unwrap();

    let bumped = find_pair_last_used(&daemon, "gh-writeback", &resource_id).await;
    assert!(bumped.is_some(), "Checkout did not bump last_used_at");

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn checkin_keeps_last_used_at_least_at_checkout_value() {
    // A wrapper run that ends with a successful Checkin necessarily
    // bumped on Checkout first. The Checkin's write also bumps, but
    // at one-second resolution within the same wall-clock second the
    // stored value can stay the same. Assert it's set.
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();
    let resource_id = seed_resource_and_pair(&daemon, "gh-writeback", "github/token").await;
    seed_writable_permission(&daemon, "gh-writeback", &resource_id).await;

    let mutated = blob_with_files_entry(".config/gh/hosts.yml", b"host: example");
    let mutated_bytes = mutated.to_canonical_bytes().unwrap();
    let socket = daemon.instance_socket.clone();
    tokio::task::spawn_blocking(move || {
        let mut channel = open_run(&socket, "gh-writeback");
        let _ = send_checkout(&mut channel);
        let resp = send_checkin(&mut channel, &mutated_bytes);
        assert!(matches!(resp, WrapperResourceResponse::CheckinOk));
        close_channel(channel);
    })
    .await
    .unwrap();

    let bumped = find_pair_last_used(&daemon, "gh-writeback", &resource_id).await;
    assert!(
        bumped.is_some(),
        "Checkin path did not leave last_used_at set"
    );

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn usage_signal_bumps_last_used() {
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();
    let resource_id = seed_resource_and_pair(&daemon, "gh-writeback", "github/token").await;
    seed_writable_permission(&daemon, "gh-writeback", &resource_id).await;

    let socket = daemon.instance_socket.clone();
    tokio::task::spawn_blocking(move || {
        let mut channel = open_run(&socket, "gh-writeback");
        // Skip Checkout — drive UsageSignal directly to isolate the
        // bump from any other backend read.
        let resp = send_usage_signal(&mut channel);
        assert!(matches!(resp, WrapperResourceResponse::UsageSignalOk));
        close_channel(channel);
    })
    .await
    .unwrap();

    let bumped = find_pair_last_used(&daemon, "gh-writeback", &resource_id).await;
    assert!(bumped.is_some(), "UsageSignal did not bump last_used_at");

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn rejected_checkin_still_bumps_last_used() {
    // Checkout against a writable grant bumps once; the operator
    // flips the grant read-only; the Checkin is rejected; ns then
    // sends UsageSignal which hits the backend's read path → second
    // bump (or same-second no-op). Either way last_used_at is set.
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();
    let resource_id = seed_resource_and_pair(&daemon, "gh-writeback", "github/token").await;
    seed_writable_permission(&daemon, "gh-writeback", &resource_id).await;

    let socket = daemon.instance_socket.clone();
    let (checkout_done_tx, checkout_done_rx) = std::sync::mpsc::channel::<()>();
    let (flip_tx, flip_rx) = std::sync::mpsc::channel::<()>();
    let mutated = blob_with_env("GH_TOKEN", b"ghp_rejected_mutation");
    let mutated_bytes = mutated.to_canonical_bytes().unwrap();
    let blocking = tokio::task::spawn_blocking(move || {
        let mut channel = open_run(&socket, "gh-writeback");
        let _ = send_checkout(&mut channel);
        checkout_done_tx.send(()).unwrap();
        flip_rx.recv().unwrap();
        let resp = send_checkin(&mut channel, &mutated_bytes);
        assert!(matches!(
            resp,
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::Unauthorized,
                ..
            }
        ));
        let resp = send_usage_signal(&mut channel);
        assert!(matches!(resp, WrapperResourceResponse::UsageSignalOk));
        close_channel(channel);
    });

    tokio::task::spawn_blocking(move || checkout_done_rx.recv().unwrap())
        .await
        .unwrap();

    daemon
        .upsert_permission(
            "test",
            &make_permission(&daemon, "gh-writeback", &resource_id, false),
        )
        .await
        .unwrap();
    flip_tx.send(()).unwrap();
    blocking.await.unwrap();

    let bumped = find_pair_last_used(&daemon, "gh-writeback", &resource_id).await;
    assert!(
        bumped.is_some(),
        "rejected-Checkin run left last_used_at unset",
    );

    daemon.stop().await.unwrap();
}
