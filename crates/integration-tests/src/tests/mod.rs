//! End-to-end integration tests.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::net::Shutdown;
use std::os::fd::AsFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::{Path, PathBuf};

use underwrap_protocol::ipc::{
    read_frame_sync, read_wrapper_resource_frame_sync, write_frame_sync,
    write_wrapper_resource_frame_sync,
};
use underwrap_protocol::message::*;
use underwrap_protocol::resource_blob::ResourceBlob;
use underwrap_protocol::scm_rights;

use crate::harness::*;

// ---------------------------------------------------------------------------
// Utility: required packaged binaries
// ---------------------------------------------------------------------------

fn binary_resolver_error(env_key: &str, detail: &str) -> String {
    format!("{env_key} {detail}; run `./crates/integration-tests/run` from the repository checkout")
}

fn resolve_cargo_bin(
    env_key: &str,
    expected_filename: &str,
    value: Option<&OsStr>,
) -> Result<PathBuf, String> {
    let value = value.ok_or_else(|| binary_resolver_error(env_key, "is not set"))?;
    let path = Path::new(value);

    if !path.is_absolute() {
        return Err(binary_resolver_error(
            env_key,
            "must be an absolute executable path",
        ));
    }
    if path.file_name() != Some(OsStr::new(expected_filename)) {
        return Err(binary_resolver_error(
            env_key,
            &format!("must end with the exact filename `{expected_filename}`"),
        ));
    }

    let metadata = path.metadata().map_err(|error| {
        binary_resolver_error(
            env_key,
            &format!("does not identify an existing executable: {error}"),
        )
    })?;
    if !metadata.is_file() {
        return Err(binary_resolver_error(
            env_key,
            "must identify a regular file",
        ));
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(binary_resolver_error(
            env_key,
            "must identify an executable file",
        ));
    }

    Ok(path.to_path_buf())
}

/// Resolve the exact packaged executable supplied by the supported test runner.
fn cargo_bin(name: &str) -> String {
    let env_key = format!("CARGO_BIN_EXE_{name}");
    let path = resolve_cargo_bin(&env_key, name, std::env::var_os(&env_key).as_deref())
        .unwrap_or_else(|message| panic!("{message}"));
    path.into_os_string().into_string().unwrap_or_else(|_| {
        panic!(
            "{}",
            binary_resolver_error(&env_key, "must be representable as UTF-8")
        )
    })
}

#[test]
fn binary_resolver_accepts_absolute_regular_executable_with_exact_name() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let binary = tempdir.path().join("underwrapd");
    std::fs::write(&binary, b"fixture").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();

    assert_eq!(
        resolve_cargo_bin(
            "CARGO_BIN_EXE_underwrapd",
            "underwrapd",
            Some(binary.as_os_str()),
        ),
        Ok(binary)
    );
}

#[test]
fn binary_resolver_rejects_every_invalid_prerequisite_shape_actionably() {
    fn assert_rejected(value: Option<&Path>, expected_detail: &str) {
        let error = resolve_cargo_bin(
            "CARGO_BIN_EXE_underwrapd",
            "underwrapd",
            value.map(Path::as_os_str),
        )
        .unwrap_err();
        assert!(error.contains("CARGO_BIN_EXE_underwrapd"), "{error}");
        assert!(error.contains(expected_detail), "{error}");
        assert!(error.contains("./crates/integration-tests/run"), "{error}");
    }

    assert_rejected(None, "is not set");
    assert_rejected(
        Some(Path::new("underwrapd")),
        "must be an absolute executable path",
    );

    let tempdir = tempfile::TempDir::new().unwrap();
    let missing = tempdir.path().join("underwrapd");
    assert_rejected(Some(&missing), "does not identify an existing executable");

    std::fs::create_dir(&missing).unwrap();
    assert_rejected(Some(&missing), "must identify a regular file");
    std::fs::remove_dir(&missing).unwrap();

    std::fs::write(&missing, b"fixture").unwrap();
    std::fs::set_permissions(&missing, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_rejected(Some(&missing), "must identify an executable file");

    let wrong_name = tempdir.path().join("underwrapd-copy");
    std::fs::write(&wrong_name, b"fixture").unwrap();
    std::fs::set_permissions(&wrong_name, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_rejected(Some(&wrong_name), "must end with the exact filename");
}

/// Find the `env` binary (absolute path). Returns None if not found.
/// Find the `echo` binary (absolute path). Returns None if not found.
fn which_echo() -> Option<String> {
    find_bin("echo")
}

/// Search common paths for a binary (NixOS-compatible).
/// Always returns an absolute path or None.
fn find_bin(name: &str) -> Option<String> {
    // Try which first.
    if let Ok(output) = std::process::Command::new("which").arg(name).output()
        && output.status.success()
    {
        let p = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if p.starts_with('/') {
            return Some(p);
        }
    }
    // Try common paths.
    for prefix in &["/usr/bin", "/bin", "/run/current-system/sw/bin"] {
        let p = format!("{prefix}/{name}");
        if std::path::Path::new(&p).exists() {
            return Some(p);
        }
    }
    // Try PATH entries directly (works in nix sandbox where 'which' may not exist).
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            let p = format!("{dir}/{name}");
            if std::path::Path::new(&p).exists() {
                return Some(p);
            }
        }
    }
    None
}

macro_rules! require_echo {
    () => {
        match which_echo() {
            Some(p) => p,
            None => {
                panic!("required binary 'echo' not found");
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn one_wrapper_config() -> HashMap<String, underwrap_protocol::config::WrapperDef> {
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "gh-authorized".to_string(),
        wrapper("/usr/bin/gh", "github/token", "GH_TOKEN"),
    );
    wrappers
}

fn checkout_resource_sync(
    socket_path: &Path,
    wrapper_name: &str,
) -> (WrapperResponse, ResourceBlob, bool) {
    let mut control = StdUnixStream::connect(socket_path).unwrap();
    write_frame_sync(
        &mut control,
        &WrapperRequest::Read {
            wrapper_name: wrapper_name.to_string(),
            variant: underwrap_protocol::variant::WrapperVariant::Real,
        },
    )
    .unwrap();
    let response: WrapperResponse = read_frame_sync(&mut control).unwrap();
    assert!(matches!(response, WrapperResponse::ReadOk { .. }));

    let fd = scm_rights::recv_fd(control.as_fd()).unwrap();
    let mut resource = StdUnixStream::from(fd);
    write_wrapper_resource_frame_sync(&mut resource, &WrapperResourceRequest::Checkout).unwrap();
    let checkout: WrapperResourceResponse =
        read_wrapper_resource_frame_sync(&mut resource).unwrap();
    let (bytes, writable) = match checkout {
        WrapperResourceResponse::CheckoutOk { blob, writable } => (blob, writable),
        other => panic!("expected CheckoutOk, got {other:?}"),
    };
    let blob = ResourceBlob::from_canonical_bytes(&bytes).unwrap();

    write_wrapper_resource_frame_sync(&mut resource, &WrapperResourceRequest::UsageSignal).unwrap();
    assert!(matches!(
        read_wrapper_resource_frame_sync::<WrapperResourceResponse>(&mut resource).unwrap(),
        WrapperResourceResponse::UsageSignalOk
    ));
    resource.shutdown(Shutdown::Both).unwrap();
    control.shutdown(Shutdown::Both).unwrap();
    drop(resource);
    drop(control);
    (response, blob, writable)
}

async fn checkout_resource(
    socket_path: std::path::PathBuf,
    wrapper_name: &'static str,
) -> (WrapperResponse, ResourceBlob, bool) {
    tokio::task::spawn_blocking(move || checkout_resource_sync(&socket_path, wrapper_name))
        .await
        .unwrap()
}

fn expect_single_env_bytes(blob: ResourceBlob) -> Vec<u8> {
    assert_eq!(blob.env.len(), 1, "expected exactly one env binding");
    let (_key, value) = blob.env.into_iter().next().unwrap();
    Vec::from(value)
}

fn expect_single_env_string(blob: ResourceBlob) -> String {
    String::from_utf8(expect_single_env_bytes(blob)).unwrap()
}

fn blob_with_exact_canonical_len(len: usize) -> ResourceBlob {
    let mut blob = ResourceBlob::empty();
    blob.files.insert(
        "padding".to_string(),
        underwrap_protocol::message::SecretBytes::from(Vec::<u8>::new()),
    );
    let base_len = blob.to_canonical_bytes().unwrap().len();
    assert!(len >= base_len);
    blob.files = std::collections::BTreeMap::from([(
        format!("padding{}", "x".repeat(len - base_len)),
        underwrap_protocol::message::SecretBytes::from(Vec::<u8>::new()),
    )]);
    assert_eq!(blob.to_canonical_bytes().unwrap().len(), len);
    blob
}

fn pad_blob_to_exact_len(mut blob: ResourceBlob, len: usize) -> ResourceBlob {
    blob.libsecret.collections.insert(
        "padding".to_string(),
        underwrap_protocol::resource_blob::Collection {
            label: String::new(),
            created: 1_700_000_000,
            modified: 1_700_000_000,
            items: std::collections::BTreeMap::new(),
        },
    );
    let base_len = blob.to_canonical_bytes().unwrap().len();
    assert!(base_len <= len);
    blob.libsecret.collections.get_mut("padding").unwrap().label = "x".repeat(len - base_len);
    assert_eq!(blob.to_canonical_bytes().unwrap().len(), len);
    blob
}

fn canonical_one_byte_over_limit() -> Vec<u8> {
    let blob = blob_with_exact_canonical_len(
        underwrap_protocol::resource_blob::MAX_CANONICAL_RESOURCE_BYTES,
    );
    let exact = blob.to_canonical_bytes().unwrap();
    let mut oversized = exact.clone();
    let marker = br#""files":{"padding"#;
    let position = oversized
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap()
        + marker.len();
    // The sole files key contains only unescaped ASCII. Extending that JSON
    // string by one ASCII byte preserves both JSON validity and JCS ordering.
    oversized.insert(position, b'x');
    assert_eq!(
        oversized.len(),
        underwrap_protocol::resource_blob::MAX_CANONICAL_RESOURCE_BYTES + 1
    );
    assert!(serde_json::from_slice::<serde_json::Value>(&oversized).is_ok());
    let mut restored = oversized.clone();
    assert_eq!(restored.remove(position), b'x');
    assert_eq!(restored, exact);
    oversized
}

async fn daemon_config_error(wrapper_def: underwrap_protocol::config::WrapperDef) -> String {
    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();
    let backend_url = "https://127.0.0.1:65535".to_string();

    let config = underwrap_protocol::config::DaemonConfig {
        backend: underwrap_protocol::config::BackendServiceConfig {
            url: backend_url,
            ca_file: Some(base.join("ca.pem")),
            client_identity_file: Some(base.join("client.identity.pem")),
        },
        instance: "local".to_string(),
        socket: base.join("instance.sock"),
        group: crate::harness::current_group_name().unwrap(),
        debug_tool_target: base.join("underwrap-debug-tool"),
        target_runtime_dir_base: "/run/user".to_string(),
        wrappers: HashMap::from([("dir-wrapper".to_string(), wrapper_def)]),
    };

    underwrapd::server::run(config, None)
        .await
        .unwrap_err()
        .to_string()
}

// ===========================================================================
// Store and read flow
// ===========================================================================

mod admin_libsecret;
mod admin_reference_integrity;
mod backend_ca;
mod client_access_override;
mod client_instances;
mod daemon;
mod directory_and_status;
mod last_used;
mod policy;
mod registry_failure_atomicity;
mod rotation;
mod runtime_api;
mod service_transport;
mod store_migration;
mod store_read;
mod wrapper_permissions;
mod wrapper_writeback;
