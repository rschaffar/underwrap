//! Axum server wiring for the admin UI.

use anyhow::{Context, Result};
use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Router, middleware};
use cookie::{Cookie, SameSite};
use serde::Deserialize;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tracing::{info, warn};

use underwrap_protocol::policy::{PolicyEffect, PolicyGrant, PolicyRule, WrapperApprovalMode};
use underwrap_protocol::resource::validate_env_name;

use crate::auth::{SESSION_COOKIE, Session, csrf_token_matches, verify_password};
use crate::backend_client::BackendClient;
use crate::config::LoadedConfig;
use crate::pages::{
    InstancePageData, instance_page, login_page, login_page_with_error, targets_page,
};

pub struct AppState {
    pub config: LoadedConfig,
    backends: std::collections::HashMap<String, BackendClient>,
}

impl AppState {
    pub fn new(config: LoadedConfig) -> Self {
        let http = reqwest::Client::new();
        let backends = config
            .backends
            .iter()
            .map(|(name, target)| {
                (
                    name.clone(),
                    BackendClient::new(&target.admin_url, &target.token, http.clone()),
                )
            })
            .collect();
        Self { config, backends }
    }

    fn backend(&self, name: &str) -> Option<&BackendClient> {
        self.backends.get(name)
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/login", get(login_form).post(login_submit))
        .route("/logout", post(logout))
        .route("/t/{target}", get(instance_view))
        .route(
            "/t/{target}/s/{store}/wrapper/{wrapper}/mode",
            post(set_wrapper_mode),
        )
        .route("/t/{target}/s/{store}/rule/upsert", post(upsert_rule))
        .route("/t/{target}/s/{store}/rule/{id}/delete", post(delete_rule))
        .route("/t/{target}/s/{store}/grant/upsert", post(upsert_grant))
        .route(
            "/t/{target}/s/{store}/grant/{id}/delete",
            post(delete_grant),
        )
        .route(
            "/t/{target}/s/{store}/secret/write-env",
            post(write_env_secret),
        )
        .route(
            "/t/{target}/s/{store}/secret/write-file",
            post(write_file_secret),
        )
        .route(
            "/t/{target}/s/{store}/secret/write-libsecret",
            post(write_libsecret_secret),
        )
        .route("/t/{target}/s/{store}/secret/delete", post(delete_secret))
        .with_state(state)
}

pub async fn run(state: Arc<AppState>, shutdown: Option<Arc<Notify>>) -> Result<()> {
    let listener = TcpListener::bind(&state.config.listen)
        .await
        .with_context(|| format!("binding admin UI listener {}", state.config.listen))?;
    info!(listen = %state.config.listen, "admin UI listening");
    let router = router(state).layer(middleware::from_fn(csp_header));
    match shutdown {
        Some(notify) => axum::serve(listener, router)
            .with_graceful_shutdown(async move { notify.notified().await })
            .await
            .context("serving admin UI")?,
        None => axum::serve(listener, router)
            .await
            .context("serving admin UI")?,
    }
    Ok(())
}

async fn csp_header(request: axum::extract::Request, next: middleware::Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; style-src 'self' 'unsafe-inline'; \
             script-src 'none'; frame-ancestors 'none'; base-uri 'self'",
        ),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

fn get_session(state: &AppState, headers: &HeaderMap) -> Option<Session> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    for raw in cookie_header.split(';') {
        if let Ok(cookie) = Cookie::parse(raw.trim())
            && cookie.name() == SESSION_COOKIE
            && let Ok(session) = Session::decode(&state.config.session_key, cookie.value())
        {
            return Some(session);
        }
    }
    None
}

fn set_session_cookie(session: &Session, state: &AppState) -> HeaderValue {
    let value = session.encode(&state.config.session_key);
    let cookie = Cookie::build((SESSION_COOKIE, value))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(false) // admin UI ships with HTTP today; operator deploys behind TLS proxy
        .path("/")
        .max_age(cookie::time::Duration::seconds(
            crate::auth::SESSION_LIFETIME_SECONDS as i64,
        ))
        .build();
    HeaderValue::from_str(&cookie.to_string()).expect("cookie encodes as ASCII")
}

fn clear_session_cookie() -> HeaderValue {
    let cookie = Cookie::build((SESSION_COOKIE, ""))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(false)
        .path("/")
        .max_age(cookie::time::Duration::seconds(0))
        .build();
    HeaderValue::from_str(&cookie.to_string()).expect("cookie encodes as ASCII")
}

fn html(body: String) -> Response {
    Html(body).into_response()
}

async fn root(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let Some(session) = get_session(&state, &headers) else {
        return Redirect::to("/login").into_response();
    };
    let mut targets: Vec<String> = state.config.backends.keys().cloned().collect();
    targets.sort();
    html(targets_page(&session.operator, &targets))
}

async fn login_form(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if get_session(&state, &headers).is_some() {
        return Redirect::to("/").into_response();
    }
    html(login_page())
}

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
}

async fn login_submit(State(state): State<Arc<AppState>>, Form(form): Form<LoginForm>) -> Response {
    let Some(phc) = state.config.operators.get(&form.username) else {
        warn!(username = %form.username, "unknown operator tried to log in");
        return (
            StatusCode::UNAUTHORIZED,
            html(login_page_with_error("invalid credentials")),
        )
            .into_response();
    };
    if verify_password(&form.password, phc).is_err() {
        warn!(username = %form.username, "password verification failed");
        return (
            StatusCode::UNAUTHORIZED,
            html(login_page_with_error("invalid credentials")),
        )
            .into_response();
    }
    let session = Session::new(&form.username);
    info!(operator = %form.username, "operator signed in");
    let mut headers = HeaderMap::new();
    headers.insert(header::SET_COOKIE, set_session_cookie(&session, &state));
    (headers, Redirect::to("/")).into_response()
}

async fn logout() -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::SET_COOKIE, clear_session_cookie());
    (headers, Redirect::to("/login")).into_response()
}

#[derive(Debug, Deserialize)]
pub struct StoreQuery {
    store: Option<String>,
}

async fn instance_view(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(target): Path<String>,
    Query(q): Query<StoreQuery>,
) -> Response {
    let Some(session) = get_session(&state, &headers) else {
        return Redirect::to("/login").into_response();
    };
    let Some(client) = state.backend(&target) else {
        return (
            StatusCode::NOT_FOUND,
            html(format!("unknown backend {target}")),
        )
            .into_response();
    };
    let stores = match client.list_stores().await {
        Ok(stores) => stores,
        Err(e) => return error_page(&session.operator, &format!("listing stores: {e}")),
    };
    if stores.is_empty() {
        return error_page(&session.operator, "backend has no stores configured");
    }
    let store = q.store.clone().unwrap_or_else(|| stores[0].clone());
    let policy = match client.get_policy(&store).await {
        Ok(p) => p,
        Err(e) => return error_page(&session.operator, &format!("loading policy: {e}")),
    };
    let secret_ids = match client.list_secrets(&store, "").await {
        Ok(ids) => ids,
        Err(e) => return error_page(&session.operator, &format!("listing secrets: {e}")),
    };

    html(instance_page(InstancePageData {
        operator: &session.operator,
        target: &target,
        store: &store,
        stores: &stores,
        policy: &policy,
        secret_ids: &secret_ids,
        csrf_token: &session.csrf_token,
        banner_ok: None,
        banner_error: None,
    }))
}

fn error_page(operator: &str, message: &str) -> Response {
    let body = format!(
        r#"<h1>Error</h1><div class="banner error">{}</div>"#,
        crate::pages::h(message),
    );
    let page = crate::pages::layout("Error", Some(operator), &body);
    (StatusCode::BAD_GATEWAY, html(page)).into_response()
}

#[must_use]
fn check_csrf(session: &Session, given: &str) -> Option<Response> {
    if csrf_token_matches(&session.csrf_token, given) {
        None
    } else {
        Some((StatusCode::FORBIDDEN, "CSRF token mismatch").into_response())
    }
}

#[derive(Debug, Deserialize)]
pub struct WrapperModeForm {
    csrf_token: String,
    mode: String,
    until_epoch: Option<String>,
    approval_key: Option<String>,
    reason: Option<String>,
}

async fn set_wrapper_mode(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((target, store, wrapper)): Path<(String, String, String)>,
    Form(form): Form<WrapperModeForm>,
) -> Response {
    let Some(session) = get_session(&state, &headers) else {
        return Redirect::to("/login").into_response();
    };
    if let Some(r) = check_csrf(&session, &form.csrf_token) {
        return r;
    }
    let Some(client) = state.backend(&target) else {
        return (StatusCode::NOT_FOUND, "unknown backend").into_response();
    };
    let mode = match form.mode.as_str() {
        "locked" => WrapperApprovalMode::Locked,
        "unlocked" => WrapperApprovalMode::Unlocked { until_epoch: None },
        "unlocked_until" => {
            let until = match form
                .until_epoch
                .as_deref()
                .unwrap_or("")
                .trim()
                .parse::<u64>()
            {
                Ok(u) => u,
                Err(_) => {
                    return error_page(
                        &session.operator,
                        "unlocked_until requires a numeric epoch",
                    );
                }
            };
            WrapperApprovalMode::Unlocked {
                until_epoch: Some(until),
            }
        }
        "per_call_approval" => {
            let key = form.approval_key.clone().unwrap_or_default();
            let key = key.trim().to_string();
            if key.is_empty() {
                return error_page(
                    &session.operator,
                    "per_call_approval requires an approval_key",
                );
            }
            WrapperApprovalMode::PerCallApproval { approval_key: key }
        }
        other => return error_page(&session.operator, &format!("unknown mode '{other}'")),
    };
    let reason = form
        .reason
        .clone()
        .filter(|s: &String| !s.trim().is_empty());
    match client
        .set_wrapper_mode(&store, &wrapper, mode, reason)
        .await
    {
        Ok(()) => {
            info!(operator = %session.operator, target = %target, store = %store, wrapper = %wrapper, "wrapper mode updated")
        }
        Err(e) => return error_page(&session.operator, &format!("setting mode: {e}")),
    }
    Redirect::to(&format!("/t/{target}?store={store}")).into_response()
}

fn parse_effect(raw: &str) -> Option<PolicyEffect> {
    match raw {
        "allow" => Some(PolicyEffect::Allow),
        "deny" => Some(PolicyEffect::Deny),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
pub struct RuleForm {
    csrf_token: String,
    id: String,
    wrapper: String,
    resource: String,
    operation: String,
    effect: String,
}

async fn upsert_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((target, store)): Path<(String, String)>,
    Form(form): Form<RuleForm>,
) -> Response {
    let Some(session) = get_session(&state, &headers) else {
        return Redirect::to("/login").into_response();
    };
    if let Some(r) = check_csrf(&session, &form.csrf_token) {
        return r;
    }
    let Some(client) = state.backend(&target) else {
        return (StatusCode::NOT_FOUND, "unknown backend").into_response();
    };
    let Some(effect) = parse_effect(&form.effect) else {
        return error_page(&session.operator, "bad effect (must be allow or deny)");
    };
    let rule = PolicyRule {
        id: form.id,
        wrapper: form.wrapper,
        resource: form.resource,
        operation: form.operation,
        effect,
    };
    if let Err(e) = client.upsert_rule(&store, rule).await {
        return error_page(&session.operator, &format!("upsert rule: {e}"));
    }
    Redirect::to(&format!("/t/{target}?store={store}")).into_response()
}

#[derive(Debug, Deserialize)]
pub struct DeleteForm {
    csrf_token: String,
}

async fn delete_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((target, store, id)): Path<(String, String, String)>,
    Form(form): Form<DeleteForm>,
) -> Response {
    let Some(session) = get_session(&state, &headers) else {
        return Redirect::to("/login").into_response();
    };
    if let Some(r) = check_csrf(&session, &form.csrf_token) {
        return r;
    }
    let Some(client) = state.backend(&target) else {
        return (StatusCode::NOT_FOUND, "unknown backend").into_response();
    };
    if let Err(e) = client.delete_rule(&store, &id).await {
        return error_page(&session.operator, &format!("delete rule: {e}"));
    }
    Redirect::to(&format!("/t/{target}?store={store}")).into_response()
}

#[derive(Debug, Deserialize)]
pub struct GrantForm {
    csrf_token: String,
    id: String,
    wrapper: String,
    resource: String,
    operation: String,
    effect: String,
    starts_at_epoch: Option<String>,
    expires_at_epoch: Option<String>,
    reason: Option<String>,
}

async fn upsert_grant(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((target, store)): Path<(String, String)>,
    Form(form): Form<GrantForm>,
) -> Response {
    let Some(session) = get_session(&state, &headers) else {
        return Redirect::to("/login").into_response();
    };
    if let Some(r) = check_csrf(&session, &form.csrf_token) {
        return r;
    }
    let Some(client) = state.backend(&target) else {
        return (StatusCode::NOT_FOUND, "unknown backend").into_response();
    };
    let Some(effect) = parse_effect(&form.effect) else {
        return error_page(&session.operator, "bad effect");
    };
    let parse_epoch = |raw: &Option<String>| -> std::result::Result<Option<u64>, String> {
        match raw.as_deref().map(str::trim) {
            Some("") | None => Ok(None),
            Some(s) => s.parse::<u64>().map(Some).map_err(|_| s.to_string()),
        }
    };
    let starts_at_epoch = match parse_epoch(&form.starts_at_epoch) {
        Ok(v) => v,
        Err(bad) => return error_page(&session.operator, &format!("bad starts_at_epoch '{bad}'")),
    };
    let expires_at_epoch = match parse_epoch(&form.expires_at_epoch) {
        Ok(v) => v,
        Err(bad) => return error_page(&session.operator, &format!("bad expires_at_epoch '{bad}'")),
    };
    let grant = PolicyGrant {
        id: form.id,
        wrapper: form.wrapper,
        resource: form.resource,
        operation: form.operation,
        effect,
        starts_at_epoch,
        expires_at_epoch,
        reason: form.reason.filter(|s: &String| !s.trim().is_empty()),
    };
    if let Err(e) = client.upsert_grant(&store, grant).await {
        return error_page(&session.operator, &format!("upsert grant: {e}"));
    }
    Redirect::to(&format!("/t/{target}?store={store}")).into_response()
}

async fn delete_grant(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((target, store, id)): Path<(String, String, String)>,
    Form(form): Form<DeleteForm>,
) -> Response {
    let Some(session) = get_session(&state, &headers) else {
        return Redirect::to("/login").into_response();
    };
    if let Some(r) = check_csrf(&session, &form.csrf_token) {
        return r;
    }
    let Some(client) = state.backend(&target) else {
        return (StatusCode::NOT_FOUND, "unknown backend").into_response();
    };
    if let Err(e) = client.delete_grant(&store, &id).await {
        return error_page(&session.operator, &format!("delete grant: {e}"));
    }
    Redirect::to(&format!("/t/{target}?store={store}")).into_response()
}

#[derive(Debug, Deserialize)]
pub struct WriteEnvSecretForm {
    csrf_token: String,
    resource_id: String,
    var_name: String,
    value: String,
}

#[derive(Debug, Deserialize)]
pub struct WriteFileSecretForm {
    csrf_token: String,
    resource_id: String,
    path: String,
    value: String,
}

#[derive(Debug, Deserialize)]
pub struct WriteLibsecretSecretForm {
    csrf_token: String,
    resource_id: String,
    item_id: String,
    value: String,
}

#[derive(Debug, Deserialize)]
pub struct DeleteSecretForm {
    csrf_token: String,
    secret_id: String,
}

fn validate_relative_item_path(path: &str) -> std::result::Result<(), &'static str> {
    if path.is_empty() || path.starts_with('/') || path.contains('\0') {
        return Err("path must be a non-empty relative path");
    }
    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err("path must be clean and must not contain empty segments");
    }
    Ok(())
}

async fn write_env_secret(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((target, store)): Path<(String, String)>,
    Form(form): Form<WriteEnvSecretForm>,
) -> Response {
    let Some(session) = get_session(&state, &headers) else {
        return Redirect::to("/login").into_response();
    };
    if let Some(r) = check_csrf(&session, &form.csrf_token) {
        return r;
    }
    let Some(client) = state.backend(&target) else {
        return (StatusCode::NOT_FOUND, "unknown backend").into_response();
    };
    let resource_id = form.resource_id.trim();
    let var_name = form.var_name.trim();
    if resource_id.is_empty() || var_name.is_empty() {
        return error_page(
            &session.operator,
            "resource_id and env var name are required",
        );
    }
    if let Err(reason) = validate_env_name(var_name) {
        return error_page(&session.operator, reason);
    }
    let secret_id = format!("{resource_id}/env/{var_name}");
    if let Err(e) = client
        .write_secret(&store, "admin-ui", &secret_id, form.value.as_bytes())
        .await
    {
        return error_page(&session.operator, &format!("write env secret: {e}"));
    }
    Redirect::to(&format!("/t/{target}?store={store}")).into_response()
}

async fn write_file_secret(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((target, store)): Path<(String, String)>,
    Form(form): Form<WriteFileSecretForm>,
) -> Response {
    let Some(session) = get_session(&state, &headers) else {
        return Redirect::to("/login").into_response();
    };
    if let Some(r) = check_csrf(&session, &form.csrf_token) {
        return r;
    }
    let Some(client) = state.backend(&target) else {
        return (StatusCode::NOT_FOUND, "unknown backend").into_response();
    };
    let resource_id = form.resource_id.trim();
    let path = form.path.trim();
    if resource_id.is_empty() || path.is_empty() {
        return error_page(&session.operator, "resource_id and file path are required");
    }
    if let Err(reason) = validate_relative_item_path(path) {
        return error_page(&session.operator, reason);
    }
    let secret_id = format!("{resource_id}/files/{path}");
    if let Err(e) = client
        .write_secret(&store, "admin-ui", &secret_id, form.value.as_bytes())
        .await
    {
        return error_page(&session.operator, &format!("write file secret: {e}"));
    }
    Redirect::to(&format!("/t/{target}?store={store}")).into_response()
}

async fn write_libsecret_secret(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((target, store)): Path<(String, String)>,
    Form(form): Form<WriteLibsecretSecretForm>,
) -> Response {
    let Some(session) = get_session(&state, &headers) else {
        return Redirect::to("/login").into_response();
    };
    if let Some(r) = check_csrf(&session, &form.csrf_token) {
        return r;
    }
    let Some(client) = state.backend(&target) else {
        return (StatusCode::NOT_FOUND, "unknown backend").into_response();
    };
    let resource_id = form.resource_id.trim();
    let item_id = form.item_id.trim();
    if resource_id.is_empty() || item_id.is_empty() {
        return error_page(&session.operator, "resource_id and item id are required");
    }
    if let Err(reason) = validate_relative_item_path(item_id) {
        return error_page(&session.operator, reason);
    }
    let secret_id = format!("{resource_id}/libsecret/{item_id}");
    if let Err(e) = client
        .write_secret(&store, "admin-ui", &secret_id, form.value.as_bytes())
        .await
    {
        return error_page(&session.operator, &format!("write libsecret secret: {e}"));
    }
    Redirect::to(&format!("/t/{target}?store={store}")).into_response()
}

async fn delete_secret(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((target, store)): Path<(String, String)>,
    Form(form): Form<DeleteSecretForm>,
) -> Response {
    let Some(session) = get_session(&state, &headers) else {
        return Redirect::to("/login").into_response();
    };
    if let Some(r) = check_csrf(&session, &form.csrf_token) {
        return r;
    }
    let Some(client) = state.backend(&target) else {
        return (StatusCode::NOT_FOUND, "unknown backend").into_response();
    };
    let secret_id = form.secret_id.trim();
    if secret_id.is_empty() {
        return error_page(&session.operator, "secret_id is required");
    }
    if let Err(e) = client.delete_secret(&store, secret_id).await {
        return error_page(&session.operator, &format!("delete secret: {e}"));
    }
    Redirect::to(&format!("/t/{target}?store={store}")).into_response()
}
