//! Optional child of client_access_override: real daemon socket and scoped-fd witnesses.
//! This does not execute underwrap-ns or prove its suppression branch runs.

use super::{
    ClientAccessOverride, PolicyDocument, RESOURCE, ResourceBlob, Result, TestDaemon,
    resource_fixture, set_file_marker, set_override, stored_matches,
};
use anyhow::ensure;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use underwrap_protocol::ipc::{
    read_frame_sync, read_wrapper_resource_frame_sync, write_frame_sync,
    write_wrapper_resource_frame_sync,
};
use underwrap_protocol::message::{
    ResourceErrorCode, WrapperRequest, WrapperResourceRequest, WrapperResourceResponse,
    WrapperResponse,
};
use underwrap_protocol::{scm_rights, variant::WrapperVariant};

async fn open_channel(socket: PathBuf) -> Result<UnixStream> {
    tokio::task::spawn_blocking(move || {
        let mut control = UnixStream::connect(socket)?;
        let timeout = Some(std::time::Duration::from_secs(5));
        control.set_read_timeout(timeout)?;
        control.set_write_timeout(timeout)?;
        write_frame_sync(&mut control, &WrapperRequest::Read {
            wrapper_name: "override-wrapper".to_string(),
            variant: WrapperVariant::Real,
        })?;
        let response: WrapperResponse = read_frame_sync(&mut control)?;
        ensure!(matches!(response, WrapperResponse::ReadOk { resource_id, .. } if resource_id == RESOURCE), "daemon did not admit wrapper read");
        let channel = UnixStream::from(scm_rights::recv_fd(control.as_fd())?);
        control.shutdown(std::net::Shutdown::Both)?;
        channel.set_read_timeout(timeout)?;
        channel.set_write_timeout(timeout)?;
        Ok(channel)
    }).await?
}

async fn exchange(
    mut channel: UnixStream,
    request: WrapperResourceRequest,
) -> Result<(UnixStream, WrapperResourceResponse)> {
    tokio::task::spawn_blocking(move || {
        write_wrapper_resource_frame_sync(&mut channel, &request)?;
        let response = read_wrapper_resource_frame_sync(&mut channel)?;
        Ok((channel, response))
    })
    .await?
}

async fn checkout(
    channel: UnixStream,
    expected_writable: bool,
) -> Result<(UnixStream, ResourceBlob)> {
    let (channel, response) = exchange(channel, WrapperResourceRequest::Checkout).await?;
    let WrapperResourceResponse::CheckoutOk { blob, writable } = response else {
        anyhow::bail!("expected scoped CheckoutOk");
    };
    ensure!(
        writable == expected_writable,
        "Checkout writable flag differs from open-time decision"
    );
    Ok((channel, ResourceBlob::from_canonical_bytes(&blob)?))
}

#[tokio::test]
async fn daemon_retains_checkout_hint_but_reauthorizes_pending_checkin() -> Result<()> {
    // The target is not executed; wrapper() configures the daemon's resource selector.
    let wrappers = std::collections::HashMap::from([(
        "override-wrapper".to_string(),
        crate::harness::wrapper("/usr/bin/unused-test-target", RESOURCE, "TEST_TOKEN"),
    )]);
    let daemon = TestDaemon::start_service_mode(wrappers, "local", "underwrap-local").await?;
    let result: Result<()> = async {
        daemon.ensure_pair("override-wrapper", RESOURCE)?;
        let initial = resource_fixture();
        daemon.put_resource_blob("test", RESOURCE, &initial)?;
        daemon.put_policy("test", &PolicyDocument::default()).await?;
        let saved = daemon.get_policy("test").await?;
        set_override(&daemon, ClientAccessOverride::Unlocked { writable: false }, &saved).await?;
        let channel = open_channel(daemon.instance_socket.clone()).await?;
        let (channel, before) = checkout(channel, false).await?;
        ensure!(before.to_canonical_bytes()? == initial.to_canonical_bytes()?, "initial Checkout changed complete resource");

        set_override(&daemon, ClientAccessOverride::Unlocked { writable: true }, &saved).await?;
        let (channel, cached) = checkout(channel, false).await?;
        ensure!(cached.to_canonical_bytes()? == before.to_canonical_bytes()?, "existing channel changed cached resource");
        // Model the caller selecting usage-only completion, not execution of namespace suppression.
        let (channel, response) = exchange(channel, WrapperResourceRequest::UsageSignal).await?;
        ensure!(matches!(response, WrapperResourceResponse::UsageSignalOk), "usage-only completion failed after writable unlock");
        drop(channel);
        stored_matches(&daemon, &initial)?;

        let channel = open_channel(daemon.instance_socket.clone()).await?;
        let (channel, mut pending) = checkout(channel, true).await?;
        set_file_marker(&mut pending, b"synthetic-daemon-pending-write");
        ensure!(pending.to_canonical_bytes()? != initial.to_canonical_bytes()?, "pending Checkin did not change content");
        set_override(&daemon, ClientAccessOverride::Locked {}, &saved).await?;
        let (channel, response) = exchange(channel, WrapperResourceRequest::Checkin {
            blob: pending.to_canonical_bytes()?,
        }).await?;
        ensure!(matches!(response, WrapperResourceResponse::Error { code: ResourceErrorCode::Unauthorized, message } if message == "client-locked"), "daemon Checkin did not surface current backend lock");
        drop(channel);
        stored_matches(&daemon, &initial)?;
        Ok(())
    }.await;
    let stopped = daemon.stop().await;
    result?;
    stopped
}
