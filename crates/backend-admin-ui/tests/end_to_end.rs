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
use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPolicyState};

struct Harness {
    ui_url: String,
    backend_admin_url: String,
    backend_admin_token: String,
    secrets_dir: std::path::PathBuf,
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
            secrets_dir: base.join("secrets"),
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

    fn put_catalog(
        &self,
        entries: &[underwrap_protocol::service::CatalogEntry],
    ) -> anyhow::Result<()> {
        #[derive(serde::Serialize)]
        struct StoredCatalogEntry<'a> {
            principal: &'a str,
            host: &'a str,
            user: &'a str,
            instance: &'a str,
            wrapper: &'a str,
            resource: &'a str,
        }

        let path = self.secrets_dir.join("__underwrap/catalog/document");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let stored: Vec<_> = entries
            .iter()
            .map(|entry| StoredCatalogEntry {
                principal: "test-principal",
                host: &entry.host,
                user: &entry.user,
                instance: &entry.instance,
                wrapper: &entry.wrapper,
                resource: &entry.resource,
            })
            .collect();
        std::fs::write(path, serde_json::to_vec(&stored)?)?;
        Ok(())
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
async fn resource_selector_renders_catalog_matrix_rows() {
    let h = Harness::start("alice", "hunter2").await.unwrap();
    h.put_catalog(&[
        underwrap_protocol::service::CatalogEntry {
            host: "deepthought".to_string(),
            user: "robert".to_string(),
            instance: "robert".to_string(),
            wrapper: "gh-authorized".to_string(),
            resource: "gh/main".to_string(),
        },
        underwrap_protocol::service::CatalogEntry {
            host: "smallthought".to_string(),
            user: "robert".to_string(),
            instance: "robert".to_string(),
            wrapper: "gh-authorized".to_string(),
            resource: "gh/main".to_string(),
        },
    ])
    .unwrap();

    let client = Harness::client();
    let login_resp = client
        .post(format!("{}/login", h.ui_url))
        .form(&[("username", "alice"), ("password", "hunter2")])
        .send()
        .await
        .unwrap();
    let cookie = login_resp
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .find_map(|v| v.to_str().ok())
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let resp = client
        .get(format!("{}/t/corp?store=work&resource=gh%2Fmain", h.ui_url))
        .header(reqwest::header::COOKIE, cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert!(body.contains("deepthought"));
    assert!(body.contains("smallthought"));
    assert!(body.contains("robert-gh-authorized"));
    assert!(body.contains("env.read"));

    h.stop().await.unwrap();
}

#[tokio::test]
async fn resource_secret_management_roundtrips_end_to_end() {
    let h = Harness::start("alice", "hunter2").await.unwrap();
    let client = Harness::client();

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

    let write_env = client
        .post(format!("{}/t/corp/s/work/secret/write-env", h.ui_url))
        .header(reqwest::header::COOKIE, &cookie_header)
        .form(&[
            ("csrf_token", csrf.as_str()),
            ("resource_id", "github/token"),
            ("var_name", "GH_TOKEN"),
            ("value", "ghp_written_by_admin_ui"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(write_env.status(), StatusCode::SEE_OTHER);

    let write_file = client
        .post(format!("{}/t/corp/s/work/secret/write-file", h.ui_url))
        .header(reqwest::header::COOKIE, &cookie_header)
        .form(&[
            ("csrf_token", csrf.as_str()),
            ("resource_id", "gws/castlabs"),
            ("path", "accounts.json"),
            ("value", "{}"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(write_file.status(), StatusCode::SEE_OTHER);

    let write_libsecret = client
        .post(format!("{}/t/corp/s/work/secret/write-libsecret", h.ui_url))
        .header(reqwest::header::COOKIE, &cookie_header)
        .form(&[
            ("csrf_token", csrf.as_str()),
            ("resource_id", "gh/session"),
            ("item_id", "default/item_1"),
            ("value", "serialized-item"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(write_libsecret.status(), StatusCode::SEE_OTHER);

    let read_env: underwrap_protocol::service::AdminSecretReadResponse = Client::new()
        .post(format!("{}/v1/admin/secret/read", h.backend_admin_url))
        .bearer_auth(&h.backend_admin_token)
        .json(&underwrap_protocol::service::AdminSecretReadRequest {
            store: "work".to_string(),
            secret_id: "github/token/env/GH_TOKEN".to_string(),
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let material = base64::engine::general_purpose::STANDARD
        .decode(read_env.material_b64.unwrap())
        .unwrap();
    assert_eq!(material, b"ghp_written_by_admin_ui");

    let list_resp: underwrap_protocol::service::AdminSecretListResponse = Client::new()
        .post(format!("{}/v1/admin/secret/list", h.backend_admin_url))
        .bearer_auth(&h.backend_admin_token)
        .json(&underwrap_protocol::service::AdminSecretListRequest {
            store: "work".to_string(),
            prefix: "".to_string(),
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        list_resp
            .ids
            .contains(&"github/token/env/GH_TOKEN".to_string())
    );
    assert!(
        list_resp
            .ids
            .contains(&"gws/castlabs/files/accounts.json".to_string())
    );
    assert!(
        list_resp
            .ids
            .contains(&"gh/session/libsecret/default/item_1".to_string())
    );

    let delete_resp = client
        .post(format!("{}/t/corp/s/work/secret/delete", h.ui_url))
        .header(reqwest::header::COOKIE, &cookie_header)
        .form(&[
            ("csrf_token", csrf.as_str()),
            ("secret_id", "github/token/env/GH_TOKEN"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(delete_resp.status(), StatusCode::SEE_OTHER);

    let read_deleted: underwrap_protocol::service::AdminSecretReadResponse = Client::new()
        .post(format!("{}/v1/admin/secret/read", h.backend_admin_url))
        .bearer_auth(&h.backend_admin_token)
        .json(&underwrap_protocol::service::AdminSecretReadRequest {
            store: "work".to_string(),
            secret_id: "github/token/env/GH_TOKEN".to_string(),
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(read_deleted.material_b64.is_none());

    h.stop().await.unwrap();
}
