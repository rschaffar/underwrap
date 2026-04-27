//! End-to-end test: start a real backendd + admin UI, log in, drive the
//! wrapper-mode editor, and verify the backend's stored policy reflects
//! the change.

use std::collections::HashMap;
use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;
use std::time::Duration;

use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHasher};
use base64::Engine;
use reqwest::redirect::Policy;
use reqwest::{Client, StatusCode};
use tempfile::TempDir;
use tokio::sync::Notify;
use underwrap_admin_ui::auth::{SESSION_COOKIE, Session};
use underwrap_admin_ui::config::{
    AdminUiConfig, BackendTargetConfig, LoadedConfig, OperatorConfig,
};
use underwrap_admin_ui::server::{AppState, run as run_ui};
use underwrap_protocol::policy::{
    OP_STORE_WRITE, PolicyDocument, PolicyEffect, WrapperApprovalMode, WrapperPolicyState,
    credential_resource,
};

struct Harness {
    ui_url: String,
    backend_admin_url: String,
    backend_admin_token: String,
    ui_shutdown: Arc<Notify>,
    backend_shutdown: Arc<Notify>,
    ui_task: tokio::task::JoinHandle<anyhow::Result<()>>,
    backend_task: tokio::task::JoinHandle<anyhow::Result<()>>,
    _tempdir: TempDir,
}

impl Harness {
    async fn start(operator: &str, password: &str) -> anyhow::Result<Self> {
        let tempdir = TempDir::new()?;
        let base = tempdir.path().to_path_buf();

        let backend_admin_token = "end-to-end-admin-token".to_string();
        let backend_admin_token_file = base.join("backend-token");
        std::fs::write(&backend_admin_token_file, &backend_admin_token)?;

        let backend_admin_port = pick_port()?;
        let backend_admin_url = format!("http://127.0.0.1:{backend_admin_port}");
        let secrets_dir = base.join("secrets");
        std::fs::create_dir_all(&secrets_dir)?;

        let backend_config = underwrap_backendd::server::BackendConfig {
            stores: HashMap::from([(
                "work".to_string(),
                underwrap_protocol::config::StorageConfig::Plain { dir: secrets_dir },
            )]),
            service: None,
            admin: Some(underwrap_backendd::server::AdminConfig {
                listen: format!("127.0.0.1:{backend_admin_port}"),
                tokens: HashMap::from([(
                    "ops".to_string(),
                    underwrap_backendd::server::AdminTokenConfig {
                        token_file: backend_admin_token_file.clone(),
                    },
                )]),
            }),
        };
        let backend_shutdown = Arc::new(Notify::new());
        let backend_shutdown_clone = Arc::clone(&backend_shutdown);
        let backend_task = tokio::spawn(async move {
            underwrap_backendd::server::run(backend_config, Some(backend_shutdown_clone)).await
        });
        wait_for_admin(&backend_admin_url, &backend_admin_token).await?;

        let hash_path = base.join("hash");
        let salt =
            SaltString::encode_b64(b"abcdefghijklmnopabcdefghijklmnop").expect("salt is valid");
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .unwrap()
            .to_string();
        std::fs::write(&hash_path, &hash)?;

        let session_key_path = base.join("session.key");
        std::fs::write(&session_key_path, [7u8; 32])?;

        let ui_port = pick_port()?;
        let ui_listen = format!("127.0.0.1:{ui_port}");
        let ui_url = format!("http://{ui_listen}");

        let ui_config = AdminUiConfig {
            listen: ui_listen.clone(),
            session_key_file: session_key_path,
            operators: HashMap::from([(
                operator.to_string(),
                OperatorConfig {
                    password_hash_file: hash_path,
                },
            )]),
            backends: HashMap::from([(
                "corp".to_string(),
                BackendTargetConfig {
                    admin_url: backend_admin_url.clone(),
                    token_file: backend_admin_token_file.clone(),
                },
            )]),
        };
        let loaded = LoadedConfig::load(&ui_config)?;
        let ui_state = Arc::new(AppState::new(loaded));
        let ui_shutdown = Arc::new(Notify::new());
        let ui_shutdown_clone = Arc::clone(&ui_shutdown);
        let ui_task = tokio::spawn(async move { run_ui(ui_state, Some(ui_shutdown_clone)).await });

        wait_for_ui(&ui_url).await?;

        Ok(Self {
            ui_url,
            backend_admin_url,
            backend_admin_token,
            ui_shutdown,
            backend_shutdown,
            ui_task,
            backend_task,
            _tempdir: tempdir,
        })
    }

    async fn stop(self) -> anyhow::Result<()> {
        self.ui_shutdown.notify_one();
        let _ = self.ui_task.await?;
        self.backend_shutdown.notify_one();
        let _ = self.backend_task.await?;
        Ok(())
    }

    fn client() -> Client {
        Client::builder().redirect(Policy::none()).build().unwrap()
    }

    async fn get_policy(&self) -> anyhow::Result<PolicyDocument> {
        let body: underwrap_protocol::service::AdminPolicyGetResponse = Client::new()
            .post(format!("{}/v1/admin/policy/get", self.backend_admin_url))
            .bearer_auth(&self.backend_admin_token)
            .json(&underwrap_protocol::service::AdminPolicyGetRequest {
                store: "work".to_string(),
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(body.document)
    }

    async fn put_policy(&self, doc: PolicyDocument) -> anyhow::Result<()> {
        Client::new()
            .post(format!("{}/v1/admin/policy/put", self.backend_admin_url))
            .bearer_auth(&self.backend_admin_token)
            .json(&underwrap_protocol::service::AdminPolicyPutRequest {
                store: "work".to_string(),
                document: doc,
            })
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}

fn pick_port() -> anyhow::Result<u16> {
    let listener = StdTcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

async fn wait_for_admin(url: &str, token: &str) -> anyhow::Result<()> {
    for _ in 0..100 {
        if let Ok(response) = Client::new()
            .get(format!("{url}/v1/admin/stores"))
            .bearer_auth(token)
            .send()
            .await
            && response.status().is_success()
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    anyhow::bail!("backend admin never became ready at {url}")
}

async fn wait_for_ui(url: &str) -> anyhow::Result<()> {
    for _ in 0..100 {
        if let Ok(response) = Client::new().get(format!("{url}/login")).send().await
            && response.status().is_success()
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    anyhow::bail!("admin UI never became ready at {url}")
}

#[tokio::test]
async fn login_rejects_wrong_password_and_accepts_right_password() {
    let h = Harness::start("alice", "hunter2").await.unwrap();

    let client = Harness::client();

    let resp = client
        .post(format!("{}/login", h.ui_url))
        .form(&[("username", "alice"), ("password", "wrong")])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = resp.text().await.unwrap();
    assert!(body.contains("invalid credentials"));

    let resp = client
        .post(format!("{}/login", h.ui_url))
        .form(&[("username", "alice"), ("password", "hunter2")])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let cookies: Vec<_> = resp
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .collect();
    assert!(
        cookies
            .iter()
            .any(|v| v.to_str().unwrap().starts_with(SESSION_COOKIE)),
        "expected session cookie in {cookies:?}"
    );

    h.stop().await.unwrap();
}

#[tokio::test]
async fn wrapper_mode_editor_updates_backend_policy() {
    let h = Harness::start("alice", "hunter2").await.unwrap();

    // Seed the backend with a wrapper state so the editor has a row to act on.
    h.put_policy(PolicyDocument {
        wrappers: vec![WrapperPolicyState {
            wrapper: "gh-authorized".to_string(),
            mode: WrapperApprovalMode::Unlocked { until_epoch: None },
            reason: None,
        }],
        rules: Vec::new(),
        grants: Vec::new(),
    })
    .await
    .unwrap();

    let client = Harness::client();

    // Log in.
    let login_resp = client
        .post(format!("{}/login", h.ui_url))
        .form(&[("username", "alice"), ("password", "hunter2")])
        .send()
        .await
        .unwrap();
    let session_cookie = login_resp
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .find_map(|v| {
            let raw = v.to_str().ok()?;
            raw.strip_prefix(&format!("{SESSION_COOKIE}="))
                .and_then(|rest| rest.split(';').next())
        })
        .expect("session cookie")
        .to_string();

    // Load the instance page to extract the CSRF token (we decode it directly
    // from the session — the server's HTML includes the same token).
    let session = Session::decode(&[7u8; 32], &session_cookie).unwrap();
    let csrf = session.csrf_token.clone();

    // POST: set wrapper mode to Locked.
    let cookie_header = format!("{SESSION_COOKIE}={session_cookie}");
    let resp = client
        .post(format!(
            "{}/t/corp/s/work/wrapper/gh-authorized/mode",
            h.ui_url
        ))
        .header(reqwest::header::COOKIE, &cookie_header)
        .form(&[
            ("csrf_token", csrf.as_str()),
            ("mode", "locked"),
            ("until_epoch", ""),
            ("approval_key", ""),
            ("reason", "testing"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    let doc = h.get_policy().await.unwrap();
    let entry = doc
        .wrappers
        .iter()
        .find(|w| w.wrapper == "gh-authorized")
        .expect("wrapper kept");
    assert_eq!(entry.mode, WrapperApprovalMode::Locked);
    assert_eq!(entry.reason.as_deref(), Some("testing"));

    // CSRF rejection: same request without the token should be 403.
    let bad = client
        .post(format!(
            "{}/t/corp/s/work/wrapper/gh-authorized/mode",
            h.ui_url
        ))
        .header(reqwest::header::COOKIE, &cookie_header)
        .form(&[
            ("csrf_token", "nope"),
            ("mode", "unlocked"),
            ("until_epoch", ""),
            ("approval_key", ""),
            ("reason", ""),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::FORBIDDEN);

    h.stop().await.unwrap();
}

/// Exercise the full admin-UI flow for credential mutation:
///
/// 1. Seed a policy document so the backend's write gate is in default-deny
///    (no policy document would auto-allow and not exercise the gate).
/// 2. Write attempt fails (no active mutation grant).
/// 3. Enable mutation via the UI (30 minutes).
/// 4. Write attempt succeeds and the backend stores the new value.
/// 5. Revoke mutation via the UI.
/// 6. Subsequent write fails again.
#[tokio::test]
async fn mutation_toggle_gates_credential_writes_end_to_end() {
    let h = Harness::start("alice", "hunter2").await.unwrap();

    // Seed an empty-but-present policy document so the default-deny posture
    // is in effect. Without a document, admin writes auto-allow for bootstrap.
    h.put_policy(PolicyDocument::default()).await.unwrap();

    let client = Harness::client();

    // Log in.
    let login_resp = client
        .post(format!("{}/login", h.ui_url))
        .form(&[("username", "alice"), ("password", "hunter2")])
        .send()
        .await
        .unwrap();
    let session_cookie = login_resp
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .find_map(|v| {
            let raw = v.to_str().ok()?;
            raw.strip_prefix(&format!("{SESSION_COOKIE}="))
                .and_then(|rest| rest.split(';').next())
        })
        .expect("session cookie")
        .to_string();
    let session = Session::decode(&[7u8; 32], &session_cookie).unwrap();
    let csrf = session.csrf_token.clone();
    let cookie_header = format!("{SESSION_COOKIE}={session_cookie}");

    // Without a grant, the write form round-trip ends on the error page.
    let write_attempt = client
        .post(format!("{}/t/corp/s/work/credential/write", h.ui_url))
        .header(reqwest::header::COOKIE, &cookie_header)
        .form(&[
            ("csrf_token", csrf.as_str()),
            ("wrapper", "gh-authorized"),
            ("secret_id", "github/token"),
            ("value", "v1"),
        ])
        .send()
        .await
        .unwrap();
    // The server renders the backend's error as a 502 page.
    assert_eq!(write_attempt.status(), StatusCode::BAD_GATEWAY);
    let body = write_attempt.text().await.unwrap();
    assert!(body.contains("denied"), "expected denial, got: {body}");

    // Enable mutation for 30 minutes.
    let enable = client
        .post(format!("{}/t/corp/s/work/mutation/enable", h.ui_url))
        .header(reqwest::header::COOKIE, &cookie_header)
        .form(&[
            ("csrf_token", csrf.as_str()),
            ("wrapper", "gh-authorized"),
            ("secret_id", "github/token"),
            ("duration", "30m"),
            ("reason", "test mutation window"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(enable.status(), StatusCode::SEE_OTHER);

    // The grant shows up in the backend policy doc with the deterministic id.
    let doc = h.get_policy().await.unwrap();
    let grant = doc
        .grants
        .iter()
        .find(|g| g.id == "mutation:gh-authorized:github/token")
        .expect("mutation grant persisted");
    assert_eq!(grant.wrapper, "gh-authorized");
    assert_eq!(grant.resource, credential_resource("github/token"));
    assert_eq!(grant.operation, OP_STORE_WRITE);
    assert_eq!(grant.effect, PolicyEffect::Allow);
    assert!(grant.expires_at_epoch.is_some());

    // Now the write succeeds.
    let write_ok = client
        .post(format!("{}/t/corp/s/work/credential/write", h.ui_url))
        .header(reqwest::header::COOKIE, &cookie_header)
        .form(&[
            ("csrf_token", csrf.as_str()),
            ("wrapper", "gh-authorized"),
            ("secret_id", "github/token"),
            ("value", "ghp_written_by_admin_ui"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(write_ok.status(), StatusCode::SEE_OTHER);

    // Confirm the stored value.
    let read_resp: underwrap_protocol::service::AdminSecretReadResponse = Client::new()
        .post(format!("{}/v1/admin/secret/read", h.backend_admin_url))
        .bearer_auth(&h.backend_admin_token)
        .json(&underwrap_protocol::service::AdminSecretReadRequest {
            store: "work".to_string(),
            secret_id: "github/token".to_string(),
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let material_b64 = read_resp.material_b64.expect("secret present");
    let material = base64::engine::general_purpose::STANDARD
        .decode(&material_b64)
        .unwrap();
    assert_eq!(material, b"ghp_written_by_admin_ui");

    // Permanent allow without confirmation is rejected.
    let permanent_no_confirm = client
        .post(format!("{}/t/corp/s/work/mutation/enable", h.ui_url))
        .header(reqwest::header::COOKIE, &cookie_header)
        .form(&[
            ("csrf_token", csrf.as_str()),
            ("wrapper", "gh-authorized"),
            ("secret_id", "github/token"),
            ("duration", "permanent"),
            ("reason", ""),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(permanent_no_confirm.status(), StatusCode::BAD_GATEWAY);
    let body = permanent_no_confirm.text().await.unwrap();
    assert!(body.contains("confirmation"), "{body}");

    // Revoke.
    let revoke = client
        .post(format!("{}/t/corp/s/work/mutation/revoke", h.ui_url))
        .header(reqwest::header::COOKIE, &cookie_header)
        .form(&[
            ("csrf_token", csrf.as_str()),
            ("grant_id", "mutation:gh-authorized:github/token"),
            ("wrapper", "gh-authorized"),
            ("secret_id", "github/token"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(revoke.status(), StatusCode::SEE_OTHER);
    let doc = h.get_policy().await.unwrap();
    assert!(
        doc.grants
            .iter()
            .all(|g| g.id != "mutation:gh-authorized:github/token"),
        "grant should be gone after revoke, got grants: {:?}",
        doc.grants,
    );

    // And subsequent writes fail again.
    let write_after_revoke = client
        .post(format!("{}/t/corp/s/work/credential/write", h.ui_url))
        .header(reqwest::header::COOKIE, &cookie_header)
        .form(&[
            ("csrf_token", csrf.as_str()),
            ("wrapper", "gh-authorized"),
            ("secret_id", "github/token"),
            ("value", "v3"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(write_after_revoke.status(), StatusCode::BAD_GATEWAY);

    h.stop().await.unwrap();
}
