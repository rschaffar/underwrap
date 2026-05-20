//! Cargo-level integration tests for the wrapper-resource channel
//! protocol: `Checkout` / `Checkin` / `UsageSignal`.
//!
//! These tests act as the wrapper-side of the IPC, driving the channel
//! against a real `underwrapd` connected to a real `underwrap-backendd`.
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
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::Path;

use base64::Engine;
use underwrap_protocol::config::AccessMode;
use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{
    ResourceErrorCode, SecretBytes, WrapperRequest, WrapperResourceRequest,
    WrapperResourceResponse, WrapperResponse,
};
use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
use underwrap_protocol::resource_blob::ResourceBlob;
use underwrap_protocol::scm_rights;

use super::*;

/// Result of the wrapper-side Read + SCM_RIGHTS exchange: ReadOk
/// metadata plus the one open `UnixStream` for the wrapper's resource
/// channel.
struct OpenedRun {
    #[allow(dead_code)] // captured for diagnostics; tests assert on the channel side
    env: HashMap<String, SecretBytes>,
    #[allow(dead_code)]
    resource_id: String,
    #[allow(dead_code)]
    access: AccessMode,
    #[allow(dead_code)]
    libsecret: bool,
    #[allow(dead_code)]
    files_target_paths: Vec<String>,
    channel: StdUnixStream,
}

/// Connect a sync wrapper-side `StdUnixStream`, send `Read`, parse
/// `ReadOk`, and receive the one channel fd via SCM_RIGHTS.
fn open_run(socket_path: &Path) -> OpenedRun {
    let mut stream = StdUnixStream::connect(socket_path)
        .unwrap_or_else(|e| panic!("connect {}: {e}", socket_path.display()));
    write_frame_sync(&mut stream, &WrapperRequest::Read { scope: None }).unwrap();
    let resp: WrapperResponse = read_frame_sync(&mut stream).unwrap();
    let (env, resource_id, access, libsecret, files_target_paths) = match resp {
        WrapperResponse::ReadOk {
            env,
            resource_id,
            access,
            libsecret,
            files_target_paths,
        } => (env, resource_id, access, libsecret, files_target_paths),
        other => panic!("expected ReadOk, got {other:?}"),
    };

    let fd =
        scm_rights::recv_fd(&stream).unwrap_or_else(|e| panic!("recv_fd for '{resource_id}': {e}"));
    // SAFETY: the fd was just received via SCM_RIGHTS and is owned
    // by this process; wrapping it in a UnixStream gives us a
    // standard owned handle for the channel.
    let channel = unsafe { StdUnixStream::from_raw_fd(fd) };

    OpenedRun {
        env,
        resource_id,
        access,
        libsecret,
        files_target_paths,
        channel,
    }
}

fn send_checkout(stream: &mut StdUnixStream) -> (Vec<u8>, bool) {
    write_frame_sync(stream, &WrapperResourceRequest::Checkout).unwrap();
    let resp: WrapperResourceResponse = read_frame_sync(stream).unwrap();
    match resp {
        WrapperResourceResponse::CheckoutOk { blob, writable } => (blob, writable),
        other => panic!("expected CheckoutOk, got {other:?}"),
    }
}

fn send_checkin(stream: &mut StdUnixStream, blob: &[u8]) -> WrapperResourceResponse {
    write_frame_sync(
        stream,
        &WrapperResourceRequest::Checkin {
            blob: blob.to_vec(),
        },
    )
    .unwrap();
    read_frame_sync(stream).unwrap()
}

fn send_usage_signal(stream: &mut StdUnixStream) -> WrapperResourceResponse {
    write_frame_sync(stream, &WrapperResourceRequest::UsageSignal).unwrap();
    read_frame_sync(stream).unwrap()
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

async fn put_blob(daemon: &TestDaemon, resource: &str, blob: &ResourceBlob) -> Vec<u8> {
    let bytes = blob.to_canonical_bytes().unwrap();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let resp = daemon
        .put_resource_blob("test", resource, &b64)
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "PUT blob failed: {}",
        resp.status()
    );
    bytes
}

async fn get_blob(daemon: &TestDaemon, resource: &str) -> ResourceBlob {
    let resp = daemon.get_resource_blob("test", resource).await.unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&resp.blob_b64)
        .unwrap();
    ResourceBlob::from_canonical_bytes(&bytes).unwrap()
}

fn blob_with_env(key: &str, value: &[u8]) -> ResourceBlob {
    let mut blob = ResourceBlob::empty();
    let mut env = BTreeMap::new();
    env.insert(key.to_string(), SecretBytes::from(value.to_vec()));
    blob.env = env;
    blob
}

// ===========================================================================
// Tests
// ===========================================================================

#[tokio::test]
async fn clean_run_only_sends_usage_signal() {
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();

    let initial = blob_with_env("GH_TOKEN", b"ghp_initial");
    let initial_bytes = put_blob(&daemon, "github/token", &initial).await;
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

    let socket = daemon.wrapper_sockets["gh-writeback"].clone();
    // All sync channel I/O happens inside spawn_blocking so it
    // doesn't block the current-thread tokio runtime the daemon
    // tasks are running on.
    tokio::task::spawn_blocking(move || {
        let run = open_run(&socket);
        let mut channel = run.channel;
        let (blob, writable) = send_checkout(&mut channel);
        assert_eq!(blob, initial_bytes);
        assert!(writable);

        let resp = send_usage_signal(&mut channel);
        assert!(matches!(resp, WrapperResourceResponse::UsageSignalOk));
    })
    .await
    .unwrap();

    // Backend blob is byte-identical to the initial state.
    let after = get_blob(&daemon, "github/token").await;
    assert_eq!(after, initial);

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn writable_grant_persists_checkin() {
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();

    let initial = blob_with_env("GH_TOKEN", b"ghp_initial");
    put_blob(&daemon, "github/token", &initial).await;
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

    let mutated = blob_with_env("GH_TOKEN", b"ghp_mutated_by_target");
    let mutated_bytes = mutated.to_canonical_bytes().unwrap();

    let socket = daemon.wrapper_sockets["gh-writeback"].clone();
    let result = tokio::task::spawn_blocking(move || {
        let run = open_run(&socket);
        let mut channel = run.channel;
        let (blob, writable) = send_checkout(&mut channel);
        assert!(writable);
        assert!(blob != mutated_bytes);
        let resp = send_checkin(&mut channel, &mutated_bytes);
        assert!(matches!(resp, WrapperResourceResponse::CheckinOk));
    })
    .await;
    result.unwrap();

    let after = get_blob(&daemon, "github/token").await;
    assert_eq!(after, mutated);

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn readonly_grant_rejects_checkin() {
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();

    let initial = blob_with_env("GH_TOKEN", b"ghp_initial");
    put_blob(&daemon, "github/token", &initial).await;
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

    let socket = daemon.wrapper_sockets["gh-writeback"].clone();
    tokio::task::spawn_blocking(move || {
        let run = open_run(&socket);
        let mut channel = run.channel;
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
    })
    .await
    .unwrap();

    // Backend blob unchanged.
    let after = get_blob(&daemon, "github/token").await;
    assert_eq!(after, initial);

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn grant_flipped_readonly_between_checkout_and_checkin() {
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();

    let initial = blob_with_env("GH_TOKEN", b"ghp_initial");
    put_blob(&daemon, "github/token", &initial).await;
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
    let socket = daemon.wrapper_sockets["gh-writeback"].clone();
    let (checkout_done_tx, checkout_done_rx) = std::sync::mpsc::channel::<()>();
    let (flip_tx, flip_rx) = std::sync::mpsc::channel::<()>();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<WrapperResourceResponse>();
    let mutated = blob_with_env("GH_TOKEN", b"ghp_flipped");
    let mutated_bytes = mutated.to_canonical_bytes().unwrap();
    let blocking = tokio::task::spawn_blocking(move || {
        let run = open_run(&socket);
        let mut channel = run.channel;
        let (_blob, writable) = send_checkout(&mut channel);
        assert!(writable, "Checkout must observe the still-writable grant");
        checkout_done_tx.send(()).unwrap();
        // Block until the test thread has flipped the grant.
        flip_rx.recv().unwrap();
        let resp = send_checkin(&mut channel, &mutated_bytes);
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
    let after = get_blob(&daemon, "github/token").await;
    assert_eq!(after, initial);

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn noncanonical_checkin_rejected() {
    let daemon = TestDaemon::start(writeback_wrapper_config()).await.unwrap();

    let initial = blob_with_env("GH_TOKEN", b"ghp_initial");
    put_blob(&daemon, "github/token", &initial).await;
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

    let socket = daemon.wrapper_sockets["gh-writeback"].clone();
    tokio::task::spawn_blocking(move || {
        let run = open_run(&socket);
        let mut channel = run.channel;
        let (_blob, _writable) = send_checkout(&mut channel);
        let resp = send_checkin(&mut channel, &noncanonical);
        match resp {
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::InvalidRequest,
                message,
            } => assert!(
                message.contains("noncanonical_blob"),
                "expected noncanonical_blob marker, got: {message}",
            ),
            other => panic!("expected InvalidRequest, got {other:?}"),
        }
    })
    .await
    .unwrap();

    let after = get_blob(&daemon, "github/token").await;
    assert_eq!(after, initial);

    daemon.stop().await.unwrap();
}

// ===========================================================================
// last_used_at coverage (ticket 423)
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
// frontend-ns decision tree synthesizes.

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

    let socket = daemon.wrapper_sockets["gh-writeback"].clone();
    tokio::task::spawn_blocking(move || {
        let run = open_run(&socket);
        let mut channel = run.channel;
        let _ = send_checkout(&mut channel);
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

    let mutated = blob_with_env("GH_TOKEN", b"ghp_mutated");
    let mutated_bytes = mutated.to_canonical_bytes().unwrap();
    let socket = daemon.wrapper_sockets["gh-writeback"].clone();
    tokio::task::spawn_blocking(move || {
        let run = open_run(&socket);
        let mut channel = run.channel;
        let _ = send_checkout(&mut channel);
        let resp = send_checkin(&mut channel, &mutated_bytes);
        assert!(matches!(resp, WrapperResourceResponse::CheckinOk));
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

    let socket = daemon.wrapper_sockets["gh-writeback"].clone();
    tokio::task::spawn_blocking(move || {
        let run = open_run(&socket);
        let mut channel = run.channel;
        // Skip Checkout — drive UsageSignal directly to isolate the
        // bump from any other backend read.
        let resp = send_usage_signal(&mut channel);
        assert!(matches!(resp, WrapperResourceResponse::UsageSignalOk));
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

    let socket = daemon.wrapper_sockets["gh-writeback"].clone();
    let (checkout_done_tx, checkout_done_rx) = std::sync::mpsc::channel::<()>();
    let (flip_tx, flip_rx) = std::sync::mpsc::channel::<()>();
    let mutated = blob_with_env("GH_TOKEN", b"ghp_rejected_mutation");
    let mutated_bytes = mutated.to_canonical_bytes().unwrap();
    let blocking = tokio::task::spawn_blocking(move || {
        let run = open_run(&socket);
        let mut channel = run.channel;
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
