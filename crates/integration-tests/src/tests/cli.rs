use super::*;

#[tokio::test]
async fn underwrapctl_store_and_status() {
    ensure_package_built("underwrapctl");
    let ctl_bin = require_bin!("underwrapctl");

    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    // Store via underwrapctl (pipe secret on stdin).
    let mut child = tokio::process::Command::new(&ctl_bin)
        .args(["--socket", daemon.control_socket.to_str().unwrap()])
        .args(["store", "github/token"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    {
        use tokio::io::AsyncWriteExt;
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(b"ghp_from_ctl\n").await.unwrap();
        stdin.shutdown().await.unwrap();
    }

    let output = child.wait_with_output().await.unwrap();
    assert!(
        output.status.success(),
        "underwrapctl store failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify via daemon read.
    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_from_ctl");

    // Query status via underwrapctl.
    let output = tokio::process::Command::new(&ctl_bin)
        .args(["--socket", daemon.control_socket.to_str().unwrap()])
        .args(["status", "gh-authorized"])
        .output()
        .await
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"stored\": true"), "stdout: {stdout}");

    daemon.stop().await.unwrap();
}

// ===========================================================================
// underwrapctl store undeclared secret → exit code 1
// ===========================================================================

#[tokio::test]
async fn underwrapctl_store_undeclared_exits_nonzero() {
    ensure_package_built("underwrapctl");
    let ctl_bin = require_bin!("underwrapctl");

    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    let mut child = tokio::process::Command::new(&ctl_bin)
        .args(["--socket", daemon.control_socket.to_str().unwrap()])
        .args(["store", "bogus/secret"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    {
        use tokio::io::AsyncWriteExt;
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(b"data\n").await.unwrap();
        stdin.shutdown().await.unwrap();
    }

    let output = child.wait_with_output().await.unwrap();
    assert!(
        !output.status.success(),
        "expected failure for undeclared secret"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("bogus/secret"), "stderr: {stderr}");

    daemon.stop().await.unwrap();
}

// ===========================================================================
// underwrapctl unlock/lock lifecycle
// ===========================================================================

#[tokio::test]
async fn underwrapctl_unlock_and_lock() {
    ensure_package_built("underwrapctl");
    let ctl_bin = require_bin!("underwrapctl");

    let mut wrappers = HashMap::new();
    wrappers.insert(
        "locked-w".to_string(),
        locked_wrapper("/usr/bin/true", "locked/tok", "LOCKED_TOKEN"),
    );
    let mut secrets = HashMap::new();
    secrets.insert("locked/tok".to_string(), secret(false));

    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();
    daemon.store_secret("locked/tok", "val").await.unwrap();

    let resp = daemon.read_request("locked-w").await.unwrap();
    match resp {
        WrapperResponse::Error { code, .. } => assert!(matches!(code, ErrorCode::Locked)),
        other => panic!("expected Locked error, got {other:?}"),
    }

    let output = tokio::process::Command::new(&ctl_bin)
        .args(["--socket", daemon.control_socket.to_str().unwrap()])
        .args(["unlock", "locked-w", "--seconds", "60"])
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "underwrapctl unlock failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ok");

    let resp = daemon.read_request("locked-w").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "val");

    let output = tokio::process::Command::new(&ctl_bin)
        .args(["--socket", daemon.control_socket.to_str().unwrap()])
        .args(["lock", "locked-w"])
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "underwrapctl lock failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ok");

    let resp = daemon.read_request("locked-w").await.unwrap();
    match resp {
        WrapperResponse::Error { code, .. } => assert!(matches!(code, ErrorCode::Locked)),
        other => panic!("expected Locked error, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn underwrapctl_unlock_unknown_wrapper_exits_nonzero() {
    ensure_package_built("underwrapctl");
    let ctl_bin = require_bin!("underwrapctl");

    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    let output = tokio::process::Command::new(&ctl_bin)
        .args(["--socket", daemon.control_socket.to_str().unwrap()])
        .args(["unlock", "no-such-wrapper", "--seconds", "60"])
        .output()
        .await
        .unwrap();

    assert!(!output.status.success(), "expected unlock to fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no-such-wrapper"), "stderr: {stderr}");

    daemon.stop().await.unwrap();
}

// ===========================================================================
// exec wrapper with missing secret → error exit

// ===========================================================================
// exec wrapper passes arguments through to target

// ===========================================================================
// exec wrapper does NOT leak secret if target doesn't use it
// (env is only set, target can choose not to print it)

// ===========================================================================
// Rapid store-read cycles (stress test for socket handling)
// ===========================================================================
