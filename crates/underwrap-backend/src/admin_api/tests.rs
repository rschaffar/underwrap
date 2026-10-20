//! HTTP requests through the production session/auth/origin/CSRF middleware.
//! Cookies are synthetic and kept in memory; assertions never print their values.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes, HttpBody};
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use hyper::body::Frame;
use tokio::sync::oneshot;
use tower::ServiceExt;
use tower_sessions::SessionStore;
use tower_sessions::cookie::{Cookie, SameSite, time::OffsetDateTime};
use tower_sessions::session::{Id, Record};

use super::{SESSION_COOKIE_NAME, admin_router};
use crate::runtime_state::ListenerState;
use crate::test_support::{admin_mut, crud_state};

const PASSWORD: &str = "synthetic-password";
const ORIGIN: &str = "https://admin.example.test";

struct Fixture {
    _tmp: tempfile::TempDir,
    state: Arc<ListenerState>,
    app: Router,
}

impl Fixture {
    fn new(lifetime: u64) -> Self {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut state = crud_state(tmp.path());
        let admin = admin_mut(&mut state);
        admin.session_lifetime_secs = lifetime;
        admin.cookies_secure = true;
        admin.csrf_config = super::csrf::build_csrf_config(true);
        admin.allowed_origins = vec![ORIGIN.into()];
        // No fixtures or live credentials are read. IDs and verifiers stay local.
        for name in ["alice", "bob"] {
            crate::users::save_user(
                &admin.users_dir,
                &crate::users::UserRecord {
                    id: crate::users::new_user_id(),
                    username: name.into(),
                    password_hash: crate::auth::hash_password(PASSWORD).unwrap(),
                    created_at: 1,
                    password_changed_at: 1,
                },
            )
            .unwrap();
        }
        let state = Arc::new(state);
        let app = admin_router(state.clone()).with_state(state.clone());
        Self {
            _tmp: tmp,
            state,
            app,
        }
    }

    async fn record(&self, client: &Client) -> Record {
        self.state
            .admin()
            .session_store
            .load(&client.id())
            .await
            .unwrap()
            .unwrap()
    }

    async fn login(&self, name: &str, password: &str) -> Client {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::post("/v1/admin/auth/login")
                    .header(header::ORIGIN, ORIGIN)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({"username": name, "password": password}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut client = Client::default();
        client.absorb(&response);
        assert!(client.session.is_some());
        client
    }
}

#[derive(Default)]
struct Client {
    session: Option<Cookie<'static>>,
    csrf_cookie: Option<Cookie<'static>>,
    csrf_token: Option<String>,
}

fn session_cookie(response: &Response) -> Option<Cookie<'static>> {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| Cookie::parse(v.to_str().unwrap().to_owned()).ok())
        .find(|c| c.name() == SESSION_COOKIE_NAME)
}

fn assert_renewal(response: &Response, lifetime: u64) {
    let cookie = session_cookie(response).expect("session renewal missing");
    assert_eq!(cookie.secure(), Some(true));
    assert_eq!(cookie.http_only(), Some(true));
    assert_eq!(cookie.same_site(), Some(SameSite::Strict));
    assert_eq!(cookie.path(), Some("/"));
    assert!(cookie.domain().is_none());
    assert_eq!(cookie.max_age().unwrap().whole_seconds(), lifetime as i64);
}

impl Client {
    fn id(&self) -> Id {
        self.session.as_ref().unwrap().value().parse().unwrap()
    }

    fn absorb(&mut self, response: &Response) {
        for value in response.headers().get_all(header::SET_COOKIE) {
            let cookie = Cookie::parse(value.to_str().unwrap().to_owned()).unwrap();
            if cookie.name() == SESSION_COOKIE_NAME {
                self.session = Some(cookie);
            } else {
                self.csrf_cookie = Some(cookie);
            }
        }
        if let Some(token) = response.headers().get(super::csrf::CSRF_HEADER_NAME) {
            self.csrf_token = Some(token.to_str().unwrap().into());
        }
    }

    async fn request(
        &self,
        app: &Router,
        method: &str,
        path: &str,
        body: serde_json::Value,
        csrf: bool,
        origin: &str,
    ) -> Response {
        let cookies = [&self.session, &self.csrf_cookie]
            .into_iter()
            .flatten()
            .map(|c| format!("{}={}", c.name(), c.value()))
            .collect::<Vec<_>>()
            .join("; ");
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::COOKIE, cookies)
            .header(header::ORIGIN, origin)
            .header(header::CONTENT_TYPE, "application/json");
        if csrf {
            request = request.header(
                super::csrf::CSRF_HEADER_NAME,
                self.csrf_token.as_ref().unwrap(),
            );
        }
        app.clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }

    async fn get(&self, app: &Router, path: &str) -> Response {
        self.request(app, "GET", path, serde_json::Value::Null, false, ORIGIN)
            .await
    }

    async fn assert_anonymous_whoami(&self, app: &Router) {
        let response = self.get(app, "/v1/admin/auth/whoami").await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let whoami: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(whoami.get("username").is_none());
    }
}

/// Pause request-body extraction after auth has loaded the session, without
/// sleeps or production hooks. Releasing the body lets the handler complete.
struct GatedBody {
    reached: Option<oneshot::Sender<()>>,
    release: oneshot::Receiver<Bytes>,
    done: bool,
}

impl HttpBody for GatedBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        if self.done {
            return Poll::Ready(None);
        }
        if let Some(reached) = self.reached.take() {
            let _ = reached.send(());
        }
        match Pin::new(&mut self.release).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(data) => {
                self.done = true;
                Poll::Ready(Some(Ok(Frame::data(data.expect("body gate released")))))
            }
        }
    }
}

#[tokio::test]
async fn session_late_response_cannot_restore_logged_out_authority() {
    let fixture = Fixture::new(60);
    let client = fixture.login("alice", PASSWORD).await;
    let bob_id = crate::users::load(&fixture.state.admin().users_dir)
        .unwrap()
        .find_by_username("bob")
        .unwrap()
        .id
        .clone();
    let cookies = [&client.session, &client.csrf_cookie]
        .into_iter()
        .flatten()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect::<Vec<_>>()
        .join("; ");
    let (reached, loaded) = oneshot::channel();
    let (release, body) = oneshot::channel();
    let request = Request::put(format!("/v1/admin/users/{bob_id}"))
        .header(header::COOKIE, cookies)
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            super::csrf::CSRF_HEADER_NAME,
            client.csrf_token.as_ref().unwrap(),
        )
        .body(Body::new(GatedBody {
            reached: Some(reached),
            release: body,
            done: false,
        }))
        .unwrap();
    let late = tokio::spawn(fixture.app.clone().oneshot(request));
    loaded.await.unwrap(); // Session/auth/CSRF passed; handler is waiting for its body.
    let logout = client
        .request(
            &fixture.app,
            "POST",
            "/v1/admin/auth/logout",
            serde_json::Value::Null,
            true,
            ORIGIN,
        )
        .await;
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    assert!(
        fixture
            .state
            .admin()
            .session_store
            .load(&client.id())
            .await
            .unwrap()
            .is_none()
    );
    release
        .send(Bytes::from_static(b"{\"username\":\"bob\"}"))
        .unwrap();
    assert_eq!(late.await.unwrap().unwrap().status(), StatusCode::OK);
    let replay = client.get(&fixture.app, "/v1/admin/stores").await;
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn session_activity_renews_cookie_and_store_then_idle_replay_expires() {
    const LIFETIME: u64 = 4;
    let fixture = Fixture::new(LIFETIME);
    let client = fixture.login("alice", PASSWORD).await;
    let original_id = client.id();
    let initial = fixture.record(&client).await.expiry_date;
    let mut previous = initial;
    // Deliberately replay the ORIGINAL cookie, independent of browser Max-Age.
    // Real wall-clock time is needed: MemoryStore uses time::now_utc, not Tokio time.
    for _ in 0..6 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let response = client.get(&fixture.app, "/v1/admin/stores").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_renewal(&response, LIFETIME);
        let renewed = session_cookie(&response).unwrap();
        assert!(renewed.value().parse::<Id>().unwrap() == original_id);
        let expiry = fixture.record(&client).await.expiry_date;
        assert!(expiry > previous, "server expiry did not advance");
        previous = expiry;
    }
    assert!(
        OffsetDateTime::now_utc() > initial,
        "must cross original deadline"
    );
    let idle = (previous - OffsetDateTime::now_utc()).unsigned_abs() + Duration::from_millis(100);
    tokio::time::sleep(idle).await;
    assert!(
        fixture
            .state
            .admin()
            .session_store
            .load(&original_id)
            .await
            .unwrap()
            .is_none()
    );
    let expired = client.get(&fixture.app, "/v1/admin/stores").await;
    assert_eq!(expired.status(), StatusCode::UNAUTHORIZED);
    assert!(
        session_cookie(&expired)
            .unwrap()
            .max_age()
            .unwrap()
            .is_zero()
    );
    // Fresh-login control rules out a broken account/backend after idle expiry.
    let fresh = fixture.login("alice", PASSWORD).await;
    assert_eq!(
        fresh.get(&fixture.app, "/v1/admin/stores").await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn session_activity_boundaries_match_pinned_middleware() {
    let fixture = Fixture::new(60);
    let client = fixture.login("alice", PASSWORD).await;
    for (method, path, body, csrf, status) in [
        (
            "POST",
            "/v1/admin/auth/login",
            serde_json::json!({"username": "alice", "password": PASSWORD}),
            false,
            StatusCode::OK,
        ),
        (
            "GET",
            "/v1/admin/auth/whoami",
            serde_json::Value::Null,
            false,
            StatusCode::OK,
        ),
        (
            "GET",
            "/v1/admin/auth/bootstrap/status",
            serde_json::Value::Null,
            false,
            StatusCode::OK,
        ),
        (
            "POST",
            "/v1/admin/auth/logout",
            serde_json::Value::Null,
            false,
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            "/v1/admin/users",
            serde_json::json!({"username":"invalid", "password":"short"}),
            true,
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let before = fixture.record(&client).await.expiry_date;
        let response = client
            .request(&fixture.app, method, path, body, csrf, ORIGIN)
            .await;
        assert_eq!(response.status(), status, "{path}");
        assert_renewal(&response, 60);
        assert!(fixture.record(&client).await.expiry_date > before);
    }
    let before = fixture.record(&client).await.expiry_date;
    let response = client
        .request(
            &fixture.app,
            "POST",
            "/v1/admin/auth/logout",
            serde_json::Value::Null,
            true,
            "https://untrusted.example.test",
        )
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(session_cookie(&response).is_none());
    assert_eq!(fixture.record(&client).await.expiry_date, before);

    // A real protected handler failure after authentication must not renew.
    // A regular file at stores_dir prevents registry persistence.
    std::fs::write(&fixture.state.admin().stores_dir, b"not a directory").unwrap();
    let response = client
        .request(
            &fixture.app,
            "POST",
            "/v1/admin/stores",
            serde_json::json!({"name": "failure-control", "backend": "plain"}),
            true,
            ORIGIN,
        )
        .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(session_cookie(&response).is_none());
    assert_eq!(fixture.record(&client).await.expiry_date, before);

    let anonymous = Client::default();
    for path in ["/v1/admin/auth/whoami", "/v1/admin/stores"] {
        let response = anonymous.get(&fixture.app, path).await;
        assert!(session_cookie(&response).is_none());
    }
}

#[tokio::test]
async fn session_renewal_preserves_revocation_paths() {
    let fixture = Fixture::new(60);
    let mut alice = fixture.login("alice", PASSWORD).await;
    let other_alice = fixture.login("alice", PASSWORD).await;
    let bob = fixture.login("bob", PASSWORD).await;
    let alice_id = crate::users::load(&fixture.state.admin().users_dir)
        .unwrap()
        .find_by_username("alice")
        .unwrap()
        .id
        .clone();

    let response = alice.request(&fixture.app, "POST", "/v1/admin/auth/password",
        serde_json::json!({"current_password": PASSWORD, "new_password": "synthetic-rotated-password"}), true, ORIGIN).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_renewal(&response, 60);
    alice.absorb(&response);
    assert_eq!(
        alice.get(&fixture.app, "/v1/admin/stores").await.status(),
        StatusCode::OK
    );
    other_alice.assert_anonymous_whoami(&fixture.app).await;
    let response = other_alice.get(&fixture.app, "/v1/admin/stores").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(
        fixture
            .state
            .admin()
            .session_store
            .load(&other_alice.id())
            .await
            .unwrap()
            .is_none()
    );

    let response = bob
        .request(
            &fixture.app,
            "POST",
            &format!("/v1/admin/users/{alice_id}/password"),
            serde_json::json!({"password": "synthetic-reset-password"}),
            true,
            ORIGIN,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    alice.assert_anonymous_whoami(&fixture.app).await;
    assert_eq!(
        alice.get(&fixture.app, "/v1/admin/stores").await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(
        fixture
            .state
            .admin()
            .session_store
            .load(&alice.id())
            .await
            .unwrap()
            .is_none()
    );

    let reset_alice = fixture.login("alice", "synthetic-reset-password").await;
    let response = bob
        .request(
            &fixture.app,
            "DELETE",
            &format!("/v1/admin/users/{alice_id}"),
            serde_json::Value::Null,
            true,
            ORIGIN,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    // Deleted-user records can remain nonempty in axum-login 0.18, but never
    // regain authority: user resolution returns None on every replay.
    for _ in 0..2 {
        reset_alice.assert_anonymous_whoami(&fixture.app).await;
        assert_eq!(
            reset_alice
                .get(&fixture.app, "/v1/admin/stores")
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    let response = bob
        .request(
            &fixture.app,
            "POST",
            "/v1/admin/auth/logout",
            serde_json::Value::Null,
            true,
            ORIGIN,
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        session_cookie(&response)
            .unwrap()
            .max_age()
            .unwrap()
            .is_zero()
    );
    assert!(
        fixture
            .state
            .admin()
            .session_store
            .load(&bob.id())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        bob.get(&fixture.app, "/v1/admin/stores").await.status(),
        StatusCode::UNAUTHORIZED
    );

    // Simulate the restart boundary by constructing fresh listener state over
    // the same persisted users, with a new MemoryStore and auth/CSRF layers.
    let bob = fixture.login("bob", PASSWORD).await;
    let restarted = Arc::new(crud_state(fixture._tmp.path()));
    let app = admin_router(restarted.clone()).with_state(restarted);
    assert_eq!(
        bob.get(&app, "/v1/admin/stores").await.status(),
        StatusCode::UNAUTHORIZED
    );
    let response = Client::default()
        .request(
            &app,
            "POST",
            "/v1/admin/auth/login",
            serde_json::json!({"username": "bob", "password": PASSWORD}),
            false,
            ORIGIN,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut fresh = Client::default();
    fresh.absorb(&response);
    assert_eq!(
        fresh.get(&app, "/v1/admin/stores").await.status(),
        StatusCode::OK
    );
}
