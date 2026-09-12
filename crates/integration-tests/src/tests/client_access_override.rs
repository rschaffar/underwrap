//! Real authenticated admin HTTPS updates followed by direct runtime mTLS calls.
//! Fixture seeding uses the existing harness; authorization never uses simulated extensions.
//! Full resource bytes are compared without formatting them in failure diagnostics.

use super::{TestDaemon, canonical_one_byte_over_limit, one_wrapper_config};
use anyhow::{Result, ensure};
use base64::Engine;
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::policy::{
    ClientAccessOverride, PolicyDocument, WrapperApprovalMode, WrapperPermission,
};
use underwrap_protocol::resource_blob::ResourceBlob;
use underwrap_protocol::service::{
    AdminClientInstanceUpdateRequest, RuntimeWrapperInvocationFinalizeRequest,
    RuntimeWrapperInvocationFinalizeResponse as Finalize, RuntimeWrapperInvocationOpenRequest,
    RuntimeWrapperInvocationOpenResponse as Open,
};
use underwrap_protocol::variant::WrapperVariant;

#[path = "client_access_override_daemon.rs"]
mod daemon;

const RESOURCE: &str = "override/resource";

fn permission(
    daemon: &TestDaemon,
    wrapper: &str,
    mode: WrapperApprovalMode,
    writable: bool,
) -> WrapperPermission {
    WrapperPermission {
        client_instance: daemon.client_instance().to_string(),
        wrapper: wrapper.to_string(),
        resource_id: RESOURCE.to_string(),
        mode,
        writable,
        reason: None,
    }
}

async fn set_override(
    daemon: &TestDaemon,
    value: ClientAccessOverride,
    saved: &PolicyDocument,
) -> Result<()> {
    let updated = daemon
        .update_client_instance(
            daemon.client_instance(),
            &AdminClientInstanceUpdateRequest {
                access_override: Some(value),
                ..Default::default()
            },
        )
        .await?;
    ensure!(
        updated.access_override == value,
        "update did not return selected override"
    );
    let listed = daemon.list_client_instances().await?.items;
    ensure!(
        listed
            .iter()
            .any(|item| item.id == daemon.client_instance() && item.access_override == value),
        "list did not publish selected override"
    );
    ensure!(
        daemon.get_policy("test").await? == *saved,
        "override rewrote saved permission rows"
    );
    Ok(())
}

async fn open(
    daemon: &TestDaemon,
    client: &reqwest::Client,
    wrapper: &str,
    variant: WrapperVariant,
) -> Result<Open> {
    let response = client
        .post(format!("{}/v1/wrapper-invocations", daemon.service_url()))
        .timeout(std::time::Duration::from_secs(5))
        .json(&RuntimeWrapperInvocationOpenRequest {
            wrapper_name: wrapper.to_string(),
            resource_name: RESOURCE.to_string(),
            variant,
        })
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "runtime open HTTP status {}",
        response.status()
    );
    Ok(response.json().await?)
}

async fn finalize(
    daemon: &TestDaemon,
    client: &reqwest::Client,
    wrapper: &str,
    variant: WrapperVariant,
    blob: Option<&ResourceBlob>,
) -> Result<Finalize> {
    let blob_b64 = blob
        .map(|blob| {
            blob.to_canonical_bytes()
                .map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes))
        })
        .transpose()?;
    finalize_encoded(daemon, client, wrapper, variant, blob_b64).await
}

async fn finalize_encoded(
    daemon: &TestDaemon,
    client: &reqwest::Client,
    wrapper: &str,
    variant: WrapperVariant,
    blob_b64: Option<String>,
) -> Result<Finalize> {
    let response = client
        .post(format!(
            "{}/v1/wrapper-invocations/finalize",
            daemon.service_url()
        ))
        .timeout(std::time::Duration::from_secs(5))
        .json(&RuntimeWrapperInvocationFinalizeRequest {
            wrapper_name: wrapper.to_string(),
            resource_name: RESOURCE.to_string(),
            variant,
            blob_b64,
        })
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "runtime finalize HTTP status {}",
        response.status()
    );
    Ok(response.json().await?)
}

fn allowed(response: Open, expected_writable: bool) -> Result<ResourceBlob> {
    let Open::Allow {
        resource_id,
        writable,
        blob_b64,
    } = response
    else {
        anyhow::bail!("expected runtime Allow");
    };
    ensure!(resource_id == RESOURCE, "unexpected resource identifier");
    ensure!(
        writable == expected_writable,
        "unexpected effective writable hint"
    );
    let bytes = base64::engine::general_purpose::STANDARD.decode(blob_b64)?;
    ResourceBlob::from_canonical_bytes(&bytes)
}

async fn denied(
    daemon: &TestDaemon,
    client: &reqwest::Client,
    wrapper: &str,
    variant: WrapperVariant,
    reason: &str,
) -> Result<()> {
    ensure!(
        matches!(open(daemon, client, wrapper, variant).await?, Open::Deny { reason: actual } if actual == reason),
        "unexpected open denial for {wrapper}"
    );
    ensure!(
        matches!(finalize(daemon, client, wrapper, variant, None).await?, Finalize::Deny { reason: actual } if actual == reason),
        "unexpected finalize denial for {wrapper}"
    );
    Ok(())
}

fn resource_fixture() -> ResourceBlob {
    let mut blob = ResourceBlob::empty();
    blob.env.insert(
        "TEST_TOKEN".to_string(),
        SecretBytes::from(b"synthetic-env".to_vec()),
    );
    blob.files.insert(
        "unchanged-file".to_string(),
        SecretBytes::from(b"synthetic-retained".to_vec()),
    );
    set_file_marker(&mut blob, b"synthetic-initial");
    blob
}

fn set_file_marker(blob: &mut ResourceBlob, bytes: &[u8]) {
    blob.files.insert(
        "override-marker".to_string(),
        SecretBytes::from(bytes.to_vec()),
    );
    // Also change libsecret metadata so the complete-resource assertions witness
    // both writable sections without copying fixture values into labels.
    let collection = blob
        .libsecret
        .collections
        .entry("override-collection".into())
        .or_insert_with(|| underwrap_protocol::resource_blob::Collection {
            label: "Integration collection".into(),
            created: 1,
            modified: 1,
            items: Default::default(),
        });
    collection.modified += 1;
}

fn stored_matches(daemon: &TestDaemon, expected: &ResourceBlob) -> Result<()> {
    // Compare the complete resource, including unchanged slots, without exposing values on failure.
    ensure!(
        daemon
            .get_resource_blob("test", RESOURCE)?
            .to_canonical_bytes()?
            == expected.to_canonical_bytes()?,
        "complete stored resource differs from expected state"
    );
    Ok(())
}

#[tokio::test]
async fn admin_override_changes_reauthorize_mtls_open_and_finalize_without_rewriting_rows()
-> Result<()> {
    let daemon =
        TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local").await?;
    let result: Result<()> = async {
        let real = WrapperVariant::Real;
        let wrappers = ["absent", "locked", "expired", "allowed"];
        for wrapper in wrappers {
            daemon.ensure_pair(wrapper, RESOURCE)?;
        }
        daemon.put_resource_blob("test", RESOURCE, &resource_fixture())?;
        daemon.put_policy("test", &PolicyDocument { permissions: vec![
            permission(&daemon, "locked", WrapperApprovalMode::Locked, false),
            permission(&daemon, "expired", WrapperApprovalMode::UnlockedUntil { until_epoch: 1 }, true),
            permission(&daemon, "allowed", WrapperApprovalMode::Unlocked, true),
        ] }).await?;
        let saved = daemon.get_policy("test").await?;
        let client = daemon.direct_service_client()?;
        ensure!(daemon.list_client_instances().await?.items.iter().any(|item| item.id == daemon.client_instance() && item.access_override == ClientAccessOverride::Individual {}), "fresh client does not default to Individual");
        for (wrapper, reason) in [("absent", "no-permission"), ("locked", "locked"), ("expired", "expired")] {
            denied(&daemon, &client, wrapper, real, reason).await?;
        }
        allowed(open(&daemon, &client, "allowed", real).await?, true)?;

        set_override(&daemon, ClientAccessOverride::Unlocked { writable: false }, &saved).await?;
        for wrapper in wrappers {
            let blob = allowed(open(&daemon, &client, wrapper, real).await?, false)?;
            ensure!(matches!(finalize(&daemon, &client, wrapper, real, Some(&blob)).await?, Finalize::Deny { reason } if reason == "not-writable"), "read-only override admitted writeback");
            ensure!(matches!(finalize(&daemon, &client, wrapper, real, None).await?, Finalize::Ok), "read-only override rejected usage-only finalize");
        }

        set_override(&daemon, ClientAccessOverride::Unlocked { writable: true }, &saved).await?;
        for wrapper in wrappers {
            let before = allowed(open(&daemon, &client, wrapper, real).await?, true)?;
            let mut changed = before.clone();
            // Distinct, nonempty synthetic file contents for every saved-row case.
            set_file_marker(&mut changed, wrapper.as_bytes());
            ensure!(changed.to_canonical_bytes()? != before.to_canonical_bytes()?, "writeback fixture did not change bytes");
            ensure!(matches!(finalize(&daemon, &client, wrapper, real, Some(&changed)).await?, Finalize::Ok), "writable override rejected registered pair");
            stored_matches(&daemon, &changed)?;
            ensure!(allowed(open(&daemon, &client, wrapper, real).await?, true)?.to_canonical_bytes()? == changed.to_canonical_bytes()?, "runtime roundtrip did not return changed complete resource");
        }
        denied(&daemon, &client, "unregistered", real, "unregistered-pair").await?;
        let prior = allowed(open(&daemon, &client, "locked", real).await?, true)?;
        let mut pending = prior.clone();
        set_file_marker(&mut pending, b"synthetic-pending-write");

        // The same mTLS client survives admin transitions; finalize must not reuse open's grant.
        set_override(&daemon, ClientAccessOverride::Locked {}, &saved).await?;
        denied(&daemon, &client, "locked", real, "client-locked").await?;
        ensure!(matches!(finalize(&daemon, &client, "locked", real, Some(&pending)).await?, Finalize::Deny { reason } if reason == "client-locked"), "lock admitted pending writeback");
        stored_matches(&daemon, &prior)?;

        set_override(&daemon, ClientAccessOverride::Unlocked { writable: false }, &saved).await?;
        ensure!(matches!(finalize(&daemon, &client, "locked", real, Some(&pending)).await?, Finalize::Deny { reason } if reason == "not-writable"), "finalize reused stale writable hint");
        stored_matches(&daemon, &prior)?;

        set_override(&daemon, ClientAccessOverride::Unlocked { writable: true }, &saved).await?;
        ensure!(matches!(finalize(&daemon, &client, "locked", real, Some(&pending)).await?, Finalize::Ok), "current writable override did not admit writeback");
        stored_matches(&daemon, &pending)?;

        // Real HTTP requests reach semantic validation under writable Unlocked even without a row.
        let mut noncanonical = pending.to_canonical_bytes()?;
        noncanonical.insert(0, b' ');
        for (bytes, expected_reason) in [
            (b"not-json".to_vec(), "decode-failed"),
            (noncanonical, "noncanonical-blob"),
            (canonical_one_byte_over_limit(), "resource-too-large"),
        ] {
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            ensure!(matches!(finalize_encoded(&daemon, &client, "absent", real, Some(encoded)).await?, Finalize::InvalidBlob { reason } if reason == expected_reason), "writable override bypassed resource validation");
            stored_matches(&daemon, &pending)?;
        }

        // Omitting access_override on a real unrelated admin update leaves it unchanged.
        let updated = daemon.update_client_instance(daemon.client_instance(), &AdminClientInstanceUpdateRequest {
            display_name: Some("Renamed integration client".to_string()),
            ..Default::default()
        }).await?;
        ensure!(updated.access_override == ClientAccessOverride::Unlocked { writable: true }, "unrelated update reset override");
        allowed(open(&daemon, &client, "absent", real).await?, true)?;

        set_override(&daemon, ClientAccessOverride::Individual {}, &saved).await?;
        for (wrapper, reason) in [("absent", "no-permission"), ("locked", "locked"), ("expired", "expired")] {
            denied(&daemon, &client, wrapper, real, reason).await?;
        }
        ensure!(matches!(finalize(&daemon, &client, "locked", real, Some(&pending)).await?, Finalize::Deny { reason } if reason == "locked"), "Individual did not restore saved mode for pending writeback");
        allowed(open(&daemon, &client, "allowed", real).await?, true)?;
        ensure!(matches!(finalize(&daemon, &client, "allowed", real, Some(&pending)).await?, Finalize::Ok), "Individual did not restore saved writable flag");
        Ok(())
    }.await;
    let stopped = daemon.stop().await;
    result?;
    stopped
}

#[tokio::test]
async fn unlocked_client_override_does_not_supply_unsafe_debug_approval_over_mtls() -> Result<()> {
    let daemon =
        TestDaemon::start_service_mode(one_wrapper_config(), "local", "underwrap-local").await?;
    let result: Result<()> = async {
        let debug = WrapperVariant::Debug;
        let wrapper = "debug-approved";
        daemon.ensure_pair(wrapper, RESOURCE)?;
        daemon.put_resource_blob("test", RESOURCE, &resource_fixture())?;
        let client = daemon.direct_service_client()?;
        daemon.put_policy("test", &PolicyDocument::default()).await?;
        let mut saved = daemon.get_policy("test").await?;
        set_override(&daemon, ClientAccessOverride::Unlocked { writable: true }, &saved).await?;
        denied(&daemon, &client, wrapper, debug, "no-permission").await?;
        for (mode, reason) in [
            (WrapperApprovalMode::Unlocked, "not-unsafe-debug"),
            (WrapperApprovalMode::UnsafeDebugUntil { until_epoch: 1 }, "expired"),
        ] {
            daemon.upsert_permission("test", &permission(&daemon, wrapper, mode, false)).await?;
            denied(&daemon, &client, wrapper, debug, reason).await?;
        }
        let until_epoch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() + 3600;
        daemon.upsert_permission("test", &permission(&daemon, wrapper, WrapperApprovalMode::UnsafeDebugUntil { until_epoch }, false)).await?;
        saved = daemon.get_policy("test").await?;
        let mut pending = allowed(open(&daemon, &client, wrapper, debug).await?, true)?;
        set_file_marker(&mut pending, b"synthetic-debug-write");
        ensure!(matches!(finalize(&daemon, &client, wrapper, debug, Some(&pending)).await?, Finalize::Ok), "writable override did not override saved debug read-only flag");
        stored_matches(&daemon, &pending)?;
        let prior = pending.clone();

        // Remove approval or replace it with an already-expired row after each successful open.
        // Expiry is deterministic admin replacement, not a wall-clock sleep/TTL-crossing witness.
        for (revoked, reason) in [
            (PolicyDocument::default(), "no-permission"),
            (PolicyDocument { permissions: vec![permission(&daemon, wrapper, WrapperApprovalMode::UnsafeDebugUntil { until_epoch: 1 }, false)] }, "expired"),
        ] {
            daemon.put_policy("test", &saved).await?;
            pending = allowed(open(&daemon, &client, wrapper, debug).await?, true)?;
            set_file_marker(&mut pending, b"synthetic-revoked-debug-write");
            daemon.put_policy("test", &revoked).await?;
            ensure!(matches!(finalize(&daemon, &client, wrapper, debug, Some(&pending)).await?, Finalize::Deny { reason: actual } if actual == reason), "finalize reused revoked debug approval");
            stored_matches(&daemon, &prior)?;
        }
        daemon.put_policy("test", &saved).await?;

        set_override(&daemon, ClientAccessOverride::Locked {}, &saved).await?;
        denied(&daemon, &client, wrapper, debug, "client-locked").await?;
        ensure!(matches!(finalize(&daemon, &client, wrapper, debug, Some(&pending)).await?, Finalize::Deny { reason } if reason == "client-locked"), "lock admitted pending debug writeback");
        stored_matches(&daemon, &prior)?;
        for value in [ClientAccessOverride::Unlocked { writable: false }, ClientAccessOverride::Individual {}] {
            set_override(&daemon, value, &saved).await?;
            allowed(open(&daemon, &client, wrapper, debug).await?, false)?;
            ensure!(matches!(finalize(&daemon, &client, wrapper, debug, Some(&pending)).await?, Finalize::Deny { reason } if reason == "not-writable"), "read-only debug admitted writeback");
            stored_matches(&daemon, &prior)?;
            ensure!(matches!(finalize(&daemon, &client, wrapper, debug, None).await?, Finalize::Ok), "active debug approval rejected usage-only finalize");
        }
        Ok(())
    }.await;
    let stopped = daemon.stop().await;
    result?;
    stopped
}
